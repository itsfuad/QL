package main

import (
	"bytes"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestValidateFormat(t *testing.T) {
	for _, format := range []string{"table", "json", "csv"} {
		if err := validateFormat(format); err != nil {
			t.Fatalf("validateFormat(%q) returned error: %v", format, err)
		}
	}

	if err := validateFormat("xml"); err == nil {
		t.Fatal("validateFormat should reject unsupported formats")
	}
}

func TestScanSourceSnapshot(t *testing.T) {
	root := t.TempDir()
	goFile := filepath.Join(root, "main.go")
	txtFile := filepath.Join(root, "notes.txt")
	if err := os.WriteFile(goFile, []byte("package main\n"), 0o644); err != nil {
		t.Fatalf("WriteFile failed: %v", err)
	}
	if err := os.WriteFile(txtFile, []byte("ignore"), 0o644); err != nil {
		t.Fatalf("WriteFile failed: %v", err)
	}

	snapshot, err := scanSourceSnapshot(root)
	if err != nil {
		t.Fatalf("scanSourceSnapshot returned error: %v", err)
	}

	if len(snapshot) != 1 {
		t.Fatalf("expected 1 source file, got %d", len(snapshot))
	}
	if _, ok := snapshot["main.go"]; !ok {
		t.Fatalf("expected main.go in snapshot: %#v", snapshot)
	}
}

func TestSnapshotsEqual(t *testing.T) {
	now := time.Now()
	later := now.Add(time.Second)

	if !snapshotsEqual(
		map[string]time.Time{"main.go": now},
		map[string]time.Time{"main.go": now},
	) {
		t.Fatal("expected equal snapshots")
	}

	if snapshotsEqual(
		map[string]time.Time{"main.go": now},
		map[string]time.Time{"main.go": later},
	) {
		t.Fatal("expected different snapshots when mtime changes")
	}
}

func TestPrintResponseTable(t *testing.T) {
	var output bytes.Buffer

	err := printResponse(&output, "table", engineResponse{
		Columns: []string{"name", "line"},
		Rows: [][]interface{}{
			{"main", float64(4)},
			{"add", float64(12)},
		},
	})
	if err != nil {
		t.Fatalf("printResponse returned error: %v", err)
	}

	expected := "name  line\n----  ----\nmain  4   \nadd   12  \n"
	if output.String() != expected {
		t.Fatalf("unexpected table output:\n%s", output.String())
	}
}

func TestPrintResponseJSON(t *testing.T) {
	var output bytes.Buffer

	err := printResponse(&output, "json", engineResponse{
		Columns: []string{"name", "line"},
		Rows: [][]interface{}{
			{"main", float64(4)},
		},
	})
	if err != nil {
		t.Fatalf("printResponse returned error: %v", err)
	}

	expected := "[{\"line\":4,\"name\":\"main\"}]\n"
	if output.String() != expected {
		t.Fatalf("unexpected json output: %q", output.String())
	}
}

func TestPrintResponseCSV(t *testing.T) {
	var output bytes.Buffer

	err := printResponse(&output, "csv", engineResponse{
		Columns: []string{"name", "line"},
		Rows: [][]interface{}{
			{"main", float64(4)},
		},
	})
	if err != nil {
		t.Fatalf("printResponse returned error: %v", err)
	}

	expected := "name,line\nmain,4\n"
	if output.String() != expected {
		t.Fatalf("unexpected csv output: %q", output.String())
	}
}

func TestRunEngineAtPath(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("shell script test is Unix-only")
	}

	dir := t.TempDir()
	enginePath := filepath.Join(dir, "ql-engine")
	script := "#!/bin/sh\ncat >/dev/null\nprintf '{\"columns\":[\"name\"],\"rows\":[[\"main\"]]}\n'\n"
	if err := os.WriteFile(enginePath, []byte(script), 0o755); err != nil {
		t.Fatalf("WriteFile failed: %v", err)
	}

	response, err := runEngineAtPath(enginePath, engineRequest{
		Query:  "SELECT * FROM functions",
		Root:   dir,
		Format: "table",
	})
	if err != nil {
		t.Fatalf("runEngineAtPath returned error: %v", err)
	}

	if len(response.Rows) != 1 || response.Rows[0][0] != "main" {
		t.Fatalf("unexpected engine response: %#v", response)
	}
}
