import * as fs from 'node:fs';
import * as path from 'node:path';
import * as cp from 'node:child_process';

import * as vscode from 'vscode';

type QueryRow = Record<string, unknown>;

interface QueryResult {
  columns: string[];
  rows: QueryRow[];
}

const DEFAULT_QUERY = 'SELECT name, file, line FROM functions ORDER BY file, line LIMIT 100';
const DEFAULT_BINARY_NAME = process.platform === 'win32' ? 'ql.exe' : 'ql';

export function activate(context: vscode.ExtensionContext): void {
  const provider = new QlResultsViewProvider(context);

  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(QlResultsViewProvider.viewType, provider),
    vscode.commands.registerCommand('ql.runQuery', async () => {
      await provider.runFromCommand();
    })
  );
}

export function deactivate(): void {}

class QlResultsViewProvider implements vscode.WebviewViewProvider {
  static readonly viewType = 'ql.resultsView';

  private view?: vscode.WebviewView;

  constructor(private readonly context: vscode.ExtensionContext) {}

  resolveWebviewView(view: vscode.WebviewView): void {
    this.view = view;
    view.webview.options = {
      enableScripts: true,
      localResourceRoots: [this.context.extensionUri],
    };
    view.webview.html = this.renderHtml(view.webview);
    view.webview.onDidReceiveMessage((message) => {
      void this.handleMessage(message);
    });
  }

  async runFromCommand(): Promise<void> {
    const query = await vscode.window.showInputBox({
      prompt: 'ql query',
      value: DEFAULT_QUERY,
    });
    if (!query) {
      return;
    }

    await this.executeQuery(query);
  }

  private async handleMessage(message: unknown): Promise<void> {
    if (!this.isMessage(message)) {
      return;
    }

    if (message.type === 'run') {
      await this.executeQuery(message.query);
      return;
    }

    if (message.type === 'open') {
      await this.openRow(message.file, message.line);
    }
  }

  private async executeQuery(query: string): Promise<void> {
    const webview = this.view?.webview;
    if (!webview) {
      return;
    }

    const workspaceRoot = this.workspaceRoot();
    if (!workspaceRoot) {
      this.postError('Open a workspace folder before running ql.');
      return;
    }

    const binary = this.findBinary();
    if (!binary) {
      this.postError('Could not find the ql binary in PATH or extension/bin/.');
      return;
    }

    this.postStatus('Running query...');

    try {
      const output = await this.spawnQuery(binary, query, workspaceRoot);
      const parsed = this.parseQueryResult(output);
      webview.postMessage({
        type: 'render',
        columns: parsed.columns,
        rows: parsed.rows,
        query,
        root: workspaceRoot,
      });
    } catch (error) {
      this.postError(error instanceof Error ? error.message : String(error));
    }
  }

  private spawnQuery(binary: string, query: string, root: string): Promise<string> {
    return new Promise((resolve, reject) => {
      const child = cp.spawn(binary, ['--format', 'json', query, root], {
        cwd: root,
        env: process.env,
      });

      let stdout = '';
      let stderr = '';

      child.stdout.setEncoding('utf8');
      child.stderr.setEncoding('utf8');

      child.stdout.on('data', (chunk: string) => {
        stdout += chunk;
      });
      child.stderr.on('data', (chunk: string) => {
        stderr += chunk;
      });
      child.on('error', reject);
      child.on('close', (code) => {
        if (code === 0) {
          resolve(stdout.trim());
          return;
        }

        reject(new Error(stderr.trim() || `ql exited with status ${code}`));
      });
    });
  }

  private parseQueryResult(output: string): QueryResult {
    if (!output) {
      return { columns: [], rows: [] };
    }

    const rows = JSON.parse(output) as QueryRow[];
    const columns = new Set<string>();
    for (const row of rows) {
      for (const key of Object.keys(row)) {
        columns.add(key);
      }
    }

    return {
      columns: [...columns],
      rows,
    };
  }

  private async openRow(file: unknown, line: unknown): Promise<void> {
    if (typeof file !== 'string' || typeof line !== 'number') {
      return;
    }

    const uri = vscode.Uri.file(path.isAbsolute(file) ? file : path.join(this.workspaceRoot() ?? '', file));
    const document = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(document, { preview: false });
    const zeroBasedLine = Math.max(0, line - 1);
    const targetLine = Math.min(zeroBasedLine, Math.max(0, document.lineCount - 1));
    const range = document.lineAt(targetLine).range;
    editor.selection = new vscode.Selection(range.start, range.start);
    editor.revealRange(range, vscode.TextEditorRevealType.InCenter);
  }

  private workspaceRoot(): string | undefined {
    return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  }

  private postError(message: string): void {
    this.view?.webview.postMessage({ type: 'error', message });
  }

  private postStatus(message: string): void {
    this.view?.webview.postMessage({ type: 'status', message });
  }

  private findBinary(): string | undefined {
    const binaryName = DEFAULT_BINARY_NAME;
    const envPath = process.env.PATH ?? '';

    for (const dir of envPath.split(path.delimiter)) {
      const candidate = path.join(dir, binaryName);
      if (this.isExecutable(candidate)) {
        return candidate;
      }
    }

    const bundled = this.context.asAbsolutePath(path.join('bin', binaryName));
    if (this.isExecutable(bundled)) {
      return bundled;
    }

    return undefined;
  }

  private isExecutable(candidate: string): boolean {
    try {
      fs.accessSync(candidate, fs.constants.X_OK);
      return true;
    } catch {
      return false;
    }
  }

  private isMessage(message: unknown): message is
    | { type: 'run'; query: string }
    | { type: 'open'; file: unknown; line: unknown } {
    if (typeof message !== 'object' || message === null) {
      return false;
    }

    const record = message as Record<string, unknown>;
    if (record.type === 'run') {
      return typeof record.query === 'string';
    }

    if (record.type === 'open') {
      return true;
    }

    return false;
  }

  private renderHtml(webview: vscode.Webview): string {
    const nonce = this.nonce();
    const escapedQuery = this.escapeHtml(DEFAULT_QUERY);

    return /* html */ `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${webview.cspSource} data:; style-src 'unsafe-inline'; script-src 'nonce-${nonce}';">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <style>
    body {
      font-family: var(--vscode-font-family);
      color: var(--vscode-foreground);
      background: var(--vscode-sideBar-background);
      margin: 0;
      padding: 12px;
    }
    .query {
      display: grid;
      gap: 8px;
      margin-bottom: 12px;
    }
    textarea {
      min-height: 120px;
      resize: vertical;
      width: 100%;
      box-sizing: border-box;
      font: inherit;
      color: inherit;
      background: var(--vscode-input-background);
      border: 1px solid var(--vscode-input-border, transparent);
      padding: 8px;
    }
    .actions {
      display: flex;
      gap: 8px;
      align-items: center;
    }
    button {
      font: inherit;
      padding: 6px 12px;
      color: var(--vscode-button-foreground);
      background: var(--vscode-button-background);
      border: none;
      cursor: pointer;
    }
    button:hover {
      background: var(--vscode-button-hoverBackground);
    }
    .meta {
      font-size: 12px;
      color: var(--vscode-descriptionForeground);
      word-break: break-word;
    }
    .status {
      margin: 8px 0 12px;
      font-size: 12px;
      color: var(--vscode-descriptionForeground);
    }
    .error {
      margin: 8px 0 12px;
      font-size: 12px;
      color: var(--vscode-errorForeground);
      white-space: pre-wrap;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      font-size: 12px;
    }
    th, td {
      text-align: left;
      padding: 6px 8px;
      border-bottom: 1px solid var(--vscode-editorWidget-border);
      vertical-align: top;
      word-break: break-word;
    }
    tr[data-clickable="true"] {
      cursor: pointer;
    }
    tr[data-clickable="true"]:hover {
      background: var(--vscode-list-hoverBackground);
    }
    .empty {
      color: var(--vscode-descriptionForeground);
      font-size: 12px;
      padding-top: 8px;
    }
  </style>
</head>
<body>
  <div class="query">
    <textarea id="query">${escapedQuery}</textarea>
    <div class="actions">
      <button id="run">Run Query</button>
      <span class="meta" id="root"></span>
    </div>
  </div>
  <div class="status" id="status">Ready.</div>
  <div class="error" id="error" hidden></div>
  <div id="results" class="empty">Run a query to see rows here.</div>

  <script nonce="${nonce}">
    const vscode = acquireVsCodeApi();
    const queryInput = document.getElementById('query');
    const runButton = document.getElementById('run');
    const status = document.getElementById('status');
    const error = document.getElementById('error');
    const results = document.getElementById('results');
    const root = document.getElementById('root');

    function runQuery() {
      vscode.postMessage({ type: 'run', query: queryInput.value });
    }

    function renderTable(columns, rows) {
      if (!rows.length) {
        results.className = 'empty';
        results.textContent = 'No rows returned.';
        return;
      }

      const header = '<tr>' + columns.map((column) => '<th>' + escapeHtml(column) + '</th>').join('') + '</tr>';
      const body = rows.map((row) => {
        const cells = columns.map((column) => {
          const value = row[column];
          return '<td>' + escapeHtml(formatValue(value)) + '</td>';
        }).join('');
        const file = row.file;
        const line = row.line;
        const clickable = typeof file === 'string' && typeof line === 'number';
        return '<tr data-clickable="' + String(clickable) + '" data-file="' + (clickable ? escapeAttr(file) : '') + '" data-line="' + (clickable ? line : '') + '">' + cells + '</tr>';
      }).join('');

      results.className = '';
      results.innerHTML = '<table><thead>' + header + '</thead><tbody>' + body + '</tbody></table>';

      for (const row of results.querySelectorAll('tr[data-clickable="true"]')) {
        row.addEventListener('click', () => {
          const file = row.getAttribute('data-file');
          const line = Number(row.getAttribute('data-line'));
          if (file && Number.isFinite(line)) {
            vscode.postMessage({ type: 'open', file, line });
          }
        });
      }
    }

    function formatValue(value) {
      if (value === null || value === undefined) {
        return '';
      }
      if (Array.isArray(value)) {
        return value.map(formatValue).join(', ');
      }
      if (typeof value === 'object') {
        return JSON.stringify(value);
      }
      return String(value);
    }

    function escapeHtml(text) {
      return text
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
    }

    function escapeAttr(text) {
      return escapeHtml(text);
    }

    runButton.addEventListener('click', runQuery);
    queryInput.addEventListener('keydown', (event) => {
      if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
        runQuery();
      }
    });

    window.addEventListener('message', (event) => {
      const message = event.data;
      if (message.type === 'status') {
        status.textContent = message.message;
        error.hidden = true;
        error.textContent = '';
        return;
      }

      if (message.type === 'error') {
        status.textContent = 'Error.';
        error.hidden = false;
        error.textContent = message.message;
        return;
      }

      if (message.type === 'render') {
        status.textContent = 'Returned ' + message.rows.length + ' row(s).';
        error.hidden = true;
        error.textContent = '';
        root.textContent = message.root;
        renderTable(message.columns, message.rows);
      }
    });
  </script>
</body>
</html>`;
  }

  private nonce(): string {
    return Math.random().toString(36).slice(2) + Math.random().toString(36).slice(2);
  }

  private escapeHtml(text: string): string {
    return text
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;')
      .replaceAll("'", '&#39;');
  }
}
