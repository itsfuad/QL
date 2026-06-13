package cli

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

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
