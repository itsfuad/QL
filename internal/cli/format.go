package cli

import (
	"encoding/csv"
	"encoding/json"
	"fmt"
	"io"
	"sort"
	"strings"
)

func validateFormat(format string) error {
	switch format {
	case "table", "json", "csv":
		return nil
	default:
		return fmt.Errorf("unsupported format %q", format)
	}
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
