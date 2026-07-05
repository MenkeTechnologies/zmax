((comment) @injection.content
 (#set! injection.language "comment"))

; ---------------------------------------------------------------------------
; SQL language injection: query strings passed to ADO.NET / Dapper / EF Core
; methods. Capture the inner string_literal_content.
((invocation_expression
  (member_access_expression
    name: (identifier) @_method)
  (argument_list
    . (argument (string_literal (string_literal_content) @injection.content))))
 (#any-of? @_method "ExecuteReader" "ExecuteNonQuery" "ExecuteScalar"
                    "ExecuteReaderAsync" "ExecuteNonQueryAsync" "ExecuteScalarAsync"
                    "FromSqlRaw" "FromSqlInterpolated" "ExecuteSqlRaw"
                    "Query" "QueryAsync" "QueryFirst" "QueryFirstOrDefault"
                    "QuerySingle" "Execute" "ExecuteAsync")
 (#set! injection.language "sql"))
