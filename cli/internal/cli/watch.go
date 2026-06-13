package cli

import (
	"fmt"
	"os"
	"time"
)

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
