package main

import (
	"bufio"
	"bytes"
	"encoding/csv"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

type engineRequest struct {
	Query  string `json:"query"`
	Root   string `json:"root"`
	Format string `json:"format"`
}

type engineResponse struct {
	Columns []string        `json:"columns"`
	Rows    [][]interface{} `json:"rows"`
	Error   string          `json:"error"`
}

func main() {
	format := flag.String("format", "table", "output format")
	langs := flag.Bool("langs", false, "list supported languages")
	watch := flag.Bool("watch", false, "rerun query on file changes")
	flag.Parse()

	if *langs {
		fmt.Println(strings.Join(supportedLanguages(), "\n"))
		return
	}

	args := flag.Args()
	if len(args) == 0 {
		exitf("error: query is required")
	}
	if len(args) > 2 {
		exitf("error: expected ql <query> [path]")
	}

	root := "."
	if len(args) == 2 {
		root = args[1]
	}

	absRoot, err := filepath.Abs(root)
	if err != nil {
		exitf("error: %v", err)
	}

	request := engineRequest{
		Query:  args[0],
		Root:   absRoot,
		Format: *format,
	}
	if err := validateFormat(request.Format); err != nil {
		exitf("error: %v", err)
	}
	if *watch {
		if err := runWatch(request); err != nil {
			exitf("error: %v", err)
		}
		return
	}

	response, err := runEngine(request)
	if err != nil {
		exitf("error: %v", err)
	}
	if response.Error != "" {
		exitf(response.Error)
	}

	if err := printResponse(os.Stdout, request.Format, response); err != nil {
		exitf("error: %v", err)
	}
}

func runWatch(request engineRequest) error {
	snapshot, err := scanSourceSnapshot(request.Root)
	if err != nil {
		return err
	}
	if err := renderQuery(request); err != nil {
		return err
	}

	ticker := time.NewTicker(500 * time.Millisecond)
	defer ticker.Stop()

	for range ticker.C {
		nextSnapshot, err := scanSourceSnapshot(request.Root)
		if err != nil {
			return err
		}
		if snapshotsEqual(snapshot, nextSnapshot) {
			continue
		}

		snapshot = nextSnapshot
		if err := renderQuery(request); err != nil {
			return err
		}
	}

	return nil
}

func renderQuery(request engineRequest) error {
	response, err := runEngine(request)
	if err != nil {
		return err
	}
	if response.Error != "" {
		return fmt.Errorf("%s", response.Error)
	}

	fmt.Print("\033[H\033[2J")
	return printResponse(os.Stdout, request.Format, response)
}

func runEngine(request engineRequest) (engineResponse, error) {
	enginePath, err := engineBinaryPath()
	if err != nil {
		return engineResponse{}, err
	}

	return runEngineAtPath(enginePath, request)
}

func runEngineAtPath(enginePath string, request engineRequest) (engineResponse, error) {
	payload, err := json.Marshal(request)
	if err != nil {
		return engineResponse{}, err
	}

	cmd := exec.Command(enginePath)
	cmd.Stdin = bytes.NewReader(append(payload, '\n'))

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		if stderr.Len() > 0 {
			return engineResponse{}, fmt.Errorf("%v: %s", err, strings.TrimSpace(stderr.String()))
		}
		return engineResponse{}, err
	}

	line, err := bufio.NewReader(&stdout).ReadString('\n')
	if err != nil && stdout.Len() == 0 {
		return engineResponse{}, fmt.Errorf("empty engine response")
	}

	var response engineResponse
	if err := json.Unmarshal([]byte(strings.TrimSpace(line)), &response); err != nil {
		return engineResponse{}, err
	}

	return response, nil
}

func validateFormat(format string) error {
	switch format {
	case "table", "json", "csv":
		return nil
	default:
		return fmt.Errorf("unsupported format %q", format)
	}
}

func engineBinaryPath() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", err
	}

	enginePath := filepath.Join(filepath.Dir(executable), "ql-engine")
	if _, err := os.Stat(enginePath); err != nil {
		return "", fmt.Errorf("ql-engine not found next to ql at %s", enginePath)
	}

	return enginePath, nil
}

func scanSourceSnapshot(root string) (map[string]time.Time, error) {
	snapshot := make(map[string]time.Time)

	err := filepath.WalkDir(root, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() {
			return nil
		}
		if !isSourceFile(path) {
			return nil
		}

		info, err := entry.Info()
		if err != nil {
			return err
		}
		relative, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		snapshot[relative] = info.ModTime()
		return nil
	})
	if err != nil {
		return nil, err
	}

	return snapshot, nil
}

func isSourceFile(path string) bool {
	switch filepath.Ext(path) {
	case ".go", ".rs", ".ts", ".py":
		return true
	default:
		return false
	}
}

func snapshotsEqual(left, right map[string]time.Time) bool {
	if len(left) != len(right) {
		return false
	}
	for path, leftTime := range left {
		rightTime, ok := right[path]
		if !ok || !leftTime.Equal(rightTime) {
			return false
		}
	}
	return true
}

func printResponse(writer io.Writer, format string, response engineResponse) error {
	switch format {
	case "json":
		return printJSON(writer, response)
	case "csv":
		return printCSV(writer, response)
	default:
		printTable(writer, response)
		return nil
	}
}

func printJSON(writer io.Writer, response engineResponse) error {
	rows := make([]map[string]interface{}, 0, len(response.Rows))
	for _, row := range response.Rows {
		item := make(map[string]interface{}, len(response.Columns))
		for i, column := range response.Columns {
			item[column] = row[i]
		}
		rows = append(rows, item)
	}

	encoder := json.NewEncoder(writer)
	return encoder.Encode(rows)
}

func printCSV(writer io.Writer, response engineResponse) error {
	csvWriter := csv.NewWriter(writer)
	if err := csvWriter.Write(response.Columns); err != nil {
		return err
	}
	for _, row := range response.Rows {
		record := make([]string, 0, len(row))
		for _, value := range row {
			record = append(record, fmt.Sprint(value))
		}
		if err := csvWriter.Write(record); err != nil {
			return err
		}
	}
	csvWriter.Flush()
	return csvWriter.Error()
}

func printTable(writer io.Writer, response engineResponse) {
	if len(response.Columns) == 0 {
		return
	}

	widths := make([]int, len(response.Columns))
	for i, column := range response.Columns {
		widths[i] = len(column)
	}
	for _, row := range response.Rows {
		for i, value := range row {
			cell := fmt.Sprint(value)
			if len(cell) > widths[i] {
				widths[i] = len(cell)
			}
		}
	}

	for i, column := range response.Columns {
		fmt.Fprintf(writer, "%-*s", widths[i], column)
		if i < len(response.Columns)-1 {
			fmt.Fprint(writer, "  ")
		}
	}
	fmt.Fprintln(writer)

	for i, width := range widths {
		fmt.Fprint(writer, strings.Repeat("-", width))
		if i < len(widths)-1 {
			fmt.Fprint(writer, "  ")
		}
	}
	fmt.Fprintln(writer)

	for _, row := range response.Rows {
		for i, value := range row {
			fmt.Fprintf(writer, "%-*v", widths[i], value)
			if i < len(row)-1 {
				fmt.Fprint(writer, "  ")
			}
		}
		fmt.Fprintln(writer)
	}
}

func supportedLanguages() []string {
	langs := []string{"go"}
	sort.Strings(langs)
	return langs
}

func exitf(format string, args ...interface{}) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
