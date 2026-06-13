package cli

import (
	"os"
	"path/filepath"
	"time"
)

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
