package store

import (
	"context"
	"database/sql"
	"fmt"
)

func Run(ctx context.Context, db *sql.DB) {
	// first-arg query method, raw string -> sql
	rows, _ := db.Query(`SELECT go_col FROM go_tbl`)

	// context method, query is 2nd arg -> sql
	_, _ = db.ExecContext(ctx, "DELETE FROM go_ctx WHERE id = 1")

	// raw-string content auto-detect -> sql
	q := `INSERT INTO go_auto (a, b) VALUES (1, 2)`

	// fmt verb string -> go-format-string (not sql)
	msg := fmt.Sprintf("row %d of goplain", 3)

	_ = rows
	_ = q
	_ = msg
}
