package cli

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

func Run() int {
	format := flag.String("format", "table", "output format")
	langs := flag.Bool("langs", false, "list supported languages")
	watch := flag.Bool("watch", false, "rerun query on file changes")
	flag.Parse()

	if *langs {
		fmt.Println(strings.Join(supportedLanguages(), "\n"))
		return 0
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
		return 0
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

	return 0
}

func exitf(format string, args ...interface{}) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
