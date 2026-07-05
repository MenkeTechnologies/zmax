([
  (line_comment)
  (multiline_comment)
 ] @injection.content
	(#set! injection.language "comment"))

; There are 3 ways to define a regex
;    - "[abc]?".toRegex()
((call_expression
	(navigation_expression
		(string_literal
		   (string_content) @injection.content)
		(navigation_suffix
			((simple_identifier) @_function
			(#eq? @_function "toRegex")))))
	(#set! injection.language "regex"))

;    - Regex("[abc]?")
((call_expression
	((simple_identifier) @_function
	(#eq? @_function "Regex"))
	(call_suffix
		(value_arguments
			(value_argument
				(string_literal
					(string_content) @injection.content)))))
	(#set! injection.language "regex"))

;    - Regex.fromLiteral("[abc]?")
((call_expression
	(navigation_expression
		((simple_identifier) @_class
		(#eq? @_class "Regex"))
		(navigation_suffix
			((simple_identifier) @_function
			(#eq? @_function "fromLiteral"))))
	(call_suffix
		(value_arguments
			(value_argument
				(string_literal
					(string_content) @injection.content)))))
	(#set! injection.language "regex"))

; ---------------------------------------------------------------------------
; SQL language injection: query strings passed to Android/JDBC query methods.
; Capture the inner string_content.
((call_expression
  (navigation_expression
    (navigation_suffix (simple_identifier) @_method))
  (call_suffix
    (value_arguments
      . (value_argument (string_literal (string_content) @injection.content)))))
 (#any-of? @_method "rawQuery" "execSQL" "query" "prepareStatement"
                    "createStatement" "createQuery" "createNativeQuery")
 (#set! injection.language "sql"))
