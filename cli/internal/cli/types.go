package cli

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
