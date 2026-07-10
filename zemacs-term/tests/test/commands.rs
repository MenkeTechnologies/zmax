use zemacs_term::application::Application;

use super::*;

mod insert;
mod movement;
mod reverse_selection_contents;
mod rotate_selection_contents;
mod write;

#[tokio::test(flavor = "multi_thread")]
async fn search_selection_detect_word_boundaries_at_eof() -> anyhow::Result<()> {
    // upstream regression test: word-boundary detection at EOF
    test((
        indoc! {"\
            #[o|]#ne
            two
            three"},
        "gej*h",
        indoc! {"\
            one
            two
            three#[
            |]#"},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_selection_duplication() -> anyhow::Result<()> {
    // Forward
    test((
        indoc! {"\
            #[lo|]#rem
            ipsum
            dolor
            "},
        "CC",
        indoc! {"\
            #(lo|)#rem
            #(ip|)#sum
            #[do|]#lor
            "},
    ))
    .await?;

    // Backward
    test((
        indoc! {"\
            #[|lo]#rem
            ipsum
            dolor
            "},
        "CC",
        indoc! {"\
            #(|lo)#rem
            #(|ip)#sum
            #[|do]#lor
            "},
    ))
    .await?;

    // Copy the selection to previous line, skipping the first line in the file
    test((
        indoc! {"\
            test
            #[testitem|]#
            "},
        "<A-C>",
        indoc! {"\
            test
            #[testitem|]#
            "},
    ))
    .await?;

    // Copy the selection to previous line, including the first line in the file
    test((
        indoc! {"\
            test
            #[test|]#
            "},
        "<A-C>",
        indoc! {"\
            #[test|]#
            #(test|)#
            "},
    ))
    .await?;

    // Copy the selection to next line, skipping the last line in the file
    test((
        indoc! {"\
            #[testitem|]#
            test
            "},
        "C",
        indoc! {"\
            #[testitem|]#
            test
            "},
    ))
    .await?;

    // Copy the selection to next line, including the last line in the file
    test((
        indoc! {"\
            #[test|]#
            test
            "},
        "C",
        indoc! {"\
            #(test|)#
            #[test|]#
            "},
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_goto_file_impl() -> anyhow::Result<()> {
    let file = tempfile::NamedTempFile::new()?;

    fn match_paths(app: &Application, matches: Vec<&str>) -> usize {
        app.editor
            .documents()
            .filter_map(|d| d.path()?.file_name())
            .filter(|n| matches.iter().any(|m| *m == n.to_string_lossy()))
            .count()
    }

    // Single selection
    test_key_sequence(
        &mut AppBuilder::new().with_file(file.path(), None).build()?,
        Some("ione.js<esc>%gf"),
        Some(&|app| {
            assert_eq!(1, match_paths(app, vec!["one.js"]));
        }),
        false,
    )
    .await?;

    // Multiple selection
    test_key_sequence(
        &mut AppBuilder::new().with_file(file.path(), None).build()?,
        Some("ione.js<ret>two.js<esc>%<A-s>gf"),
        Some(&|app| {
            assert_eq!(2, match_paths(app, vec!["one.js", "two.js"]));
        }),
        false,
    )
    .await?;

    // Cursor on first quote
    test_key_sequence(
        &mut AppBuilder::new().with_file(file.path(), None).build()?,
        Some("iimport 'one.js'<esc>B;gf"),
        Some(&|app| {
            assert_eq!(1, match_paths(app, vec!["one.js"]));
        }),
        false,
    )
    .await?;

    // Cursor on last quote
    test_key_sequence(
        &mut AppBuilder::new().with_file(file.path(), None).build()?,
        Some("iimport 'one.js'<esc>bgf"),
        Some(&|app| {
            assert_eq!(1, match_paths(app, vec!["one.js"]));
        }),
        false,
    )
    .await?;

    // ';' is behind the path
    test_key_sequence(
        &mut AppBuilder::new().with_file(file.path(), None).build()?,
        Some("iimport 'one.js';<esc>B;gf"),
        Some(&|app| {
            assert_eq!(1, match_paths(app, vec!["one.js"]));
        }),
        false,
    )
    .await?;

    // allow numeric values in path
    test_key_sequence(
        &mut AppBuilder::new().with_file(file.path(), None).build()?,
        Some("iimport 'one123.js'<esc>B;gf"),
        Some(&|app| {
            assert_eq!(1, match_paths(app, vec!["one123.js"]));
        }),
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_multi_selection_paste() -> anyhow::Result<()> {
    test((
        indoc! {"\
            #[|lorem]#
            #(|ipsum)#
            #(|dolor)#
            "},
        "yp",
        // vim-faithful paste: Normal-mode paste rests a bare cursor on the last
        // pasted char per selection, rather than selecting the pasted copy.
        indoc! {"\
            loremlore#[m|]#
            ipsumipsu#(m|)#
            dolordolo#(r|)#
            "},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_multi_selection_shell_commands() -> anyhow::Result<()> {
    // pipe
    test((
        indoc! {"\
            #[|lorem]#
            #(|ipsum)#
            #(|dolor)#
            "},
        "|echo foo<ret>",
        indoc! {"\
            #[|foo]#
            #(|foo)#
            #(|foo)#"
        },
    ))
    .await?;

    // insert-output
    test((
        indoc! {"\
            #[|lorem]#
            #(|ipsum)#
            #(|dolor)#
            "},
        "!echo foo<ret>",
        indoc! {"\
            #[|foo]#lorem
            #(|foo)#ipsum
            #(|foo)#dolor
            "},
    ))
    .await?;

    // append-output
    test((
        indoc! {"\
            #[|lorem]#
            #(|ipsum)#
            #(|dolor)#
            "},
        "<A-!>echo foo<ret>",
        indoc! {"\
            lorem#[|foo]#
            ipsum#(|foo)#
            dolor#(|foo)#
            "},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_undo_redo() -> anyhow::Result<()> {
    // A jumplist selection is created at a point which is undone.
    //
    // * 2[<space>   Add two newlines at line start. We're now on line 3.
    // * <C-s>       Save the selection on line 3 in the jumplist.
    // * u           Undo the two newlines. We're now on line 1.
    // * <C-o><C-i>  Jump forward an back again in the jumplist. This would panic
    //               if the jumplist were not being updated correctly.
    test((
        "#[|]#",
        "2[<space><C-s>u<C-o><C-i>",
        "#[|]#",
        LineFeedHandling::AsIs,
    ))
    .await?;

    // A jumplist selection is passed through an edit and then an undo and then a redo.
    //
    // * [<space>    Add a newline at line start. We're now on line 2.
    // * <C-s>       Save the selection on line 2 in the jumplist.
    // * kd          Delete line 1. The jumplist selection should be adjusted to the new line 1.
    // * uU          Undo and redo the `kd` edit.
    // * <C-o>       Jump back in the jumplist. This would panic if the jumplist were not being
    //               updated correctly.
    // * <C-i>       Jump forward to line 1.
    test((
        "#[|]#",
        "[<space><C-s>kduU<C-o><C-i>",
        "#[|]#",
        LineFeedHandling::AsIs,
    ))
    .await?;

    // In this case we 'redo' manually to ensure that the transactions are composing correctly.
    test((
        "#[|]#",
        "[<space>u[<space>u",
        "#[|]#",
        LineFeedHandling::AsIs,
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_extend_line() -> anyhow::Result<()> {
    // extend with line selected then count
    test((
        indoc! {"\
            #[l|]#orem
            ipsum
            dolor
            
            "},
        "x2x",
        indoc! {"\
            #[lorem
            ipsum
            dolor\n|]#
            
            "},
    ))
    .await?;

    // extend with count on partial selection
    test((
        indoc! {"\
            #[l|]#orem
            ipsum
            
            "},
        "2x",
        indoc! {"\
            #[lorem
            ipsum\n|]#
            
            "},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_character_info() -> anyhow::Result<()> {
    // UTF-8, single byte
    test_key_sequence(
        &mut helpers::AppBuilder::new().build()?,
        Some("ih<esc>h:char<ret>"),
        Some(&|app| {
            assert_eq!(
                r#""h" (U+0068) Dec 104 Hex 68"#,
                app.editor.get_status().unwrap().0
            );
        }),
        false,
    )
    .await?;

    // UTF-8, multi-byte
    test_key_sequence(
        &mut helpers::AppBuilder::new().build()?,
        Some("ië<esc>h:char<ret>"),
        Some(&|app| {
            assert_eq!(
                r#""ë" (U+0065 U+0308) Hex 65 + cc 88"#,
                app.editor.get_status().unwrap().0
            );
        }),
        false,
    )
    .await?;

    // Multiple characters displayed as one, escaped characters
    test_key_sequence(
        &mut helpers::AppBuilder::new().build()?,
        Some(":line<minus>ending crlf<ret>:char<ret>"),
        Some(&|app| {
            assert_eq!(
                r#""\r\n" (U+000d U+000a) Hex 0d + 0a"#,
                app.editor.get_status().unwrap().0
            );
        }),
        false,
    )
    .await?;

    // Non-UTF-8
    test_key_sequence(
        &mut helpers::AppBuilder::new().build()?,
        Some(":encoding ascii<ret>ih<esc>h:char<ret>"),
        Some(&|app| {
            assert_eq!(r#""h" Dec 104 Hex 68"#, app.editor.get_status().unwrap().0);
        }),
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_delete_char_backward() -> anyhow::Result<()> {
    // don't panic when deleting overlapping ranges
    test(("#(x|)# #[x|]#", "c<space><backspace><esc>", "#[\n|]#")).await?;
    test((
        "#( |)##( |)#a#( |)#axx#[x|]#a",
        "li<backspace><esc>",
        "#(a|)##(|a)#xx#[|a]#",
    ))
    .await?;

    Ok(())
}

// Cursor behavior is different when the text is created in the buffer vs loaded from a file.
// This test will not work for reproducing the crash or verifying the result after the fix.
// // #[tokio::test(flavor = "multi_thread")]
// async fn test_try_restore_indent() -> anyhow::Result<()> {
//     test((" #[ |]#foo\na#( |)#bar\n", "o<C-u><esc>", " foo\n#[\n|]#a bar\n#(\n|)#")).await?;
//     Ok(())
// }

#[tokio::test(flavor = "multi_thread")]
async fn test_try_restore_indent() -> anyhow::Result<()> {
    // Bug: 15228 try_restore_indent uses primary cursor position for all selections,
    // causing invalid range errors when multiple cursors are on different lines
    let file = temp_file_with_contents("  foo\na bar\n")?;
    test_key_sequence(
        &mut AppBuilder::new().with_file(file.path(), None).build()?,
        Some("jl<A-C>o<C-u><esc>"),
        None,
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_delete_word_backward() -> anyhow::Result<()> {
    // don't panic when deleting overlapping ranges
    test(("fo#[o|]#ba#(r|)#", "a<C-w><esc>", "#[\n|]#")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_delete_word_forward() -> anyhow::Result<()> {
    // don't panic when deleting overlapping ranges
    test(("fo#[o|]#b#(|ar)#", "i<A-d><esc>", "fo#[\n|]#")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_delete_char_forward() -> anyhow::Result<()> {
    test((
        indoc! {"\
                #[abc|]#def
                #(abc|)#ef
                #(abc|)#f
                #(abc|)#
            "},
        "a<del><esc>",
        indoc! {"\
                #[abc|]#ef
                #(abc|)#f
                #(abc|)#
                #(abc|)#
            "},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_insert_with_indent() -> anyhow::Result<()> {
    const INPUT: &str = indoc! { "
        #[f|]#n foo() {
            if let Some(_) = None {

            }
         
        }

        fn bar() {

        }
        "
    };

    // insert_at_line_start
    test((
        INPUT,
        ":lang rust<ret>%<A-s>I",
        indoc! { "
            #[f|]#n foo() {
                #(i|)#f let Some(_) = None {
                    #(\n|)#
                #(}|)#
            #( |)#
            #(}|)#
            #(\n|)#
            #(f|)#n bar() {
                #(\n|)#
            #(}|)#
            "
        },
    ))
    .await?;

    // insert_at_line_end
    test((
        INPUT,
        ":lang rust<ret>%<A-s>A",
        indoc! { "
            fn foo() {#[\n|]#
                if let Some(_) = None {#(\n|)#
                    #(\n|)#
                }#(\n|)#
             #(\n|)#
            }#(\n|)#
            #(\n|)#
            fn bar() {#(\n|)#
                #(\n|)#
            }#(\n|)#
            "
        },
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_join_selections() -> anyhow::Result<()> {
    // normal join
    test((
        indoc! {"\
            #[a|]#bc
            def
        "},
        "J",
        indoc! {"\
            #[a|]#bc def
        "},
    ))
    .await?;

    // join with empty line
    test((
        indoc! {"\
            #[a|]#bc

            def
        "},
        "JJ",
        indoc! {"\
            #[a|]#bc def
        "},
    ))
    .await?;

    // join with additional space in non-empty line
    test((
        indoc! {"\
            #[a|]#bc

                def
        "},
        "JJ",
        indoc! {"\
            #[a|]#bc def
        "},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_join_selections_space() -> anyhow::Result<()> {
    // join with empty lines panic
    test((
        indoc! {"\
            #[a

            b

            c

            d

            e|]#
        "},
        "<A-J>",
        indoc! {"\
            a#[ |]#b#( |)#c#( |)#d#( |)#e
        "},
    ))
    .await?;

    // normal join
    test((
        indoc! {"\
            #[a|]#bc
            def
        "},
        "<A-J>",
        indoc! {"\
            abc#[ |]#def
        "},
    ))
    .await?;

    // join with empty line
    test((
        indoc! {"\
            #[a|]#bc

            def
        "},
        "<A-J>",
        indoc! {"\
            #[a|]#bc
            def
        "},
    ))
    .await?;

    // join with additional space in non-empty line
    test((
        indoc! {"\
            #[a|]#bc

                def
        "},
        "<A-J><A-J>",
        indoc! {"\
            abc#[ |]#def
        "},
    ))
    .await?;

    // join with retained trailing spaces
    test((
        indoc! {"\
            #[aaa   

            bb  

            c |]#
        "},
        "<A-J>",
        indoc! {"\
            aaa   #[ |]#bb  #( |)#c 
        "},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_join_selections_comment() -> anyhow::Result<()> {
    test((
        indoc! {"\
            /// #[a|]#bc
            /// def
        "},
        ":lang rust<ret>J",
        indoc! {"\
            /// #[a|]#bc def
        "},
    ))
    .await?;

    // Only join if the comment token matches the previous line.
    test((
        indoc! {"\
            #[| // a
            // b
            /// c
            /// d
            e
            /// f
            // g]#
        "},
        ":lang rust<ret>J",
        indoc! {"\
            #[| // a b /// c d e f // g]#
        "},
    ))
    .await?;

    test((
        "#[|\t// Join comments
\t// with indent]#",
        ":lang go<ret>J",
        "#[|\t// Join comments with indent]#",
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_toggle_comments_inside_comment_injection() -> anyhow::Result<()> {
    // A `//` line comment's text is injected as the `comment` language, which has no
    // comment-tokens of its own. With the cursor inside the comment, toggling must
    // resolve tokens from the enclosing language and un-comment the line,
    // not fall back to the hardcoded default `#`.
    test((
        indoc! {"\
            // #[a|]#bc
        "},
        ":lang rust<ret><C-c>",
        indoc! {"\
            #[a|]#bc
        "},
    ))
    .await?;

    // A `///` doc comment's text is injected as markdown (no line comment token of
    // its own). Toggling must strip the whole `///` marker via Rust's tokens rather
    // than insert a markdown `<!-- -->` inside or leave a stray `/`.
    test((
        indoc! {"\
            /// #[a|]#bc
        "},
        ":lang rust<ret><C-c>",
        indoc! {"\
            #[a|]#bc
        "},
    ))
    .await?;

    // Likewise for the `//!` inner doc comment marker.
    test((
        indoc! {"\
            //! #[a|]#bc
        "},
        ":lang rust<ret><C-c>",
        indoc! {"\
            #[a|]#bc
        "},
    ))
    .await?;

    // Commenting a normal code line still uses the top-level language's token
    // (no injection layer at the cursor), no regression for the common case.
    test((
        indoc! {"\
            #[l|]#et x = 5;
        "},
        ":lang rust<ret><C-c>",
        indoc! {"\
            // #[l|]#et x = 5;
        "},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_read_file() -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    let contents_to_read = "some contents";
    let output_file = helpers::temp_file_with_contents(contents_to_read)?;

    test_key_sequence(
        &mut helpers::AppBuilder::new()
            .with_file(file.path(), None)
            .build()?,
        Some(&format!(":r {:?}<ret><esc>:w<ret>", output_file.path())),
        Some(&|app| {
            assert!(!app.editor.is_err(), "error: {:?}", app.editor.get_status());
        }),
        false,
    )
    .await?;

    let expected_contents = LineFeedHandling::Native.apply(contents_to_read);
    helpers::assert_file_has_content(&mut file, &expected_contents)?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn surround_delete() -> anyhow::Result<()> {
    // Test `surround_delete` when head < anchor
    test(("(#[|  ]#)", "mdm", "#[|  ]#")).await?;
    test(("(#[|  ]#)", "md(", "#[|  ]#")).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn surround_replace_ts() -> anyhow::Result<()> {
    const INPUT: &str = r#"\
fn foo() {
    if let Some(_) = None {
        testing!("f#[|o]#o)");
    }
}
"#;
    test((
        INPUT,
        ":lang rust<ret>mrm'",
        r#"\
fn foo() {
    if let Some(_) = None {
        testing!('f#[|o]#o)');
    }
}
"#,
    ))
    .await?;

    test((
        INPUT,
        ":lang rust<ret>3mrm[",
        r#"\
fn foo() {
    if let Some(_) = None [
        testing!("f#[|o]#o)");
    ]
}
"#,
    ))
    .await?;

    test((
        INPUT,
        ":lang rust<ret>2mrm{",
        r#"\
fn foo() {
    if let Some(_) = None {
        testing!{"f#[|o]#o)"};
    }
}
"#,
    ))
    .await?;

    test((
        indoc! {"\
            #[a
            b
            c
            d
            e|]#
            f
            "},
        "s\\n<ret>r,",
        "a#[,|]#b#(,|)#c#(,|)#d#(,|)#e\nf\n",
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn macro_play_within_macro_record() -> anyhow::Result<()> {
    // upstream regression test: playing a macro while recording a macro
    //
    // * `"aQihello<esc>Q` record a macro to register 'a' which inserts "hello"
    // * `Q"aq<space>world<esc>Q` record a macro to the default macro register which plays the
    //   macro in register 'a' and then inserts " world"
    // * `%d` clear the buffer
    // * `q` replay the macro in the default macro register
    // * `i<ret>` add a newline at the end
    //
    // The inner macro in register 'a' should replay within the outer macro exactly once to insert
    // "hello world".
    test((
        indoc! {"\
            #[|]#
        "},
        r#""aQihello<esc>QQ"aqi<space>world<esc>Q%dqi<ret>"#,
        indoc! {"\
            hello world
            #[|]#"},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn global_search_with_multibyte_chars() -> anyhow::Result<()> {
    // Assert that `zemacs_term::commands::global_search` handles multibyte characters correctly.
    test((
        indoc! {"\
            // Hello world!
            // #[|
            ]#
            "},
        // start global search
        " /«十分に長い マルチバイトキャラクター列» で検索<ret><esc>",
        indoc! {"\
            // Hello world!
            // #[|
            ]#
            "},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn align_selections_with_varying_columns() -> anyhow::Result<()> {
    test((
        indoc! {r"
            #[|]#I    I  II I
            IIIIIIIII
            IIIII
            IIIIIIIII
        "},
        r"%sI<ret>&gg",
        indoc! {r"
            #[I|]#    I  II I
            I    I  II IIIII
            I    I  II I
            I    I  II IIIII
        "},
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_trailing_whitespace_removes_eol_spaces() -> anyhow::Result<()> {
    test((
        "#[|h]#ello   \nworld\t\nkept\n",
        ":delete-trailing-whitespace<ret>",
        "#[|h]#ello\nworld\nkept\n",
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_line_middle() -> anyhow::Result<()> {
    test((
        "#[|a]#bc\ndef\n",
        ":duplicate-line<ret>",
        "#[|a]#bc\nabc\ndef\n",
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_line_last_no_trailing_newline() -> anyhow::Result<()> {
    test((
        "abc\n#[|x]#yz",
        ":duplicate-line<ret>",
        "abc\n#[|x]#yz\nxyz",
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_line_down_middle() -> anyhow::Result<()> {
    test((
        "#[|a]#aa\nbbb\nccc\n",
        ":move-line-down<ret>",
        "bbb\n#[a|]#aa\nccc\n",
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_line_up_middle() -> anyhow::Result<()> {
    test((
        "aaa\n#[|b]#bb\nccc\n",
        ":move-line-up<ret>",
        "#[b|]#bb\naaa\nccc\n",
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_line_down_into_last_no_newline() -> anyhow::Result<()> {
    test(("#[|a]#aa\nbbb", ":move-line-down<ret>", "bbb\n#[a|]#aa")).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn transpose_chars_basic() -> anyhow::Result<()> {
    test(("a#[|b]#c\n", ":transpose-chars<ret>", "ba#[c|]#\n")).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn transpose_chars_noop_at_buffer_start() -> anyhow::Result<()> {
    test(("#[|a]#bc\n", ":transpose-chars<ret>", "#[|a]#bc\n")).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn transpose_words_basic() -> anyhow::Result<()> {
    test((
        "#[|f]#oo bar!\n",
        ":transpose-words<ret>",
        "bar foo#[!|]#\n",
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn transpose_words_noop_single_word() -> anyhow::Result<()> {
    test(("#[|f]#oo\n", ":transpose-words<ret>", "#[|f]#oo\n")).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn uniquify_lines_removes_duplicates() -> anyhow::Result<()> {
    test((
        "#[|a]#\nb\na\nc\nb\n",
        ":uniquify-lines<ret>",
        "#[|a]#\nb\nc\n",
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_blank_lines_collapses_runs() -> anyhow::Result<()> {
    test((
        "#[|a]#\n\n\nb\n",
        ":delete-blank-lines<ret>",
        "#[|a]#\n\nb\n",
    ))
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn just_one_space_collapses_run() -> anyhow::Result<()> {
    test(("a #[| ]# b\n", ":just-one-space<ret>", "a #[b|]#\n")).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn change_case_camel_to_snake() -> anyhow::Result<()> {
    test(("#[|f]#ooBar\n", ":change-case snake<ret>", "#[f|]#oo_bar\n")).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn change_case_snake_to_camel() -> anyhow::Result<()> {
    test(("#[|f]#oo_bar\n", ":change-case camel<ret>", "#[f|]#ooBar\n")).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn cycle_case_snake_to_camel() -> anyhow::Result<()> {
    test(("#[|f]#oo_bar\n", ":cycle-case<ret>", "#[f|]#ooBar\n")).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn split_line_keeps_cursor() -> anyhow::Result<()> {
    test(("ab#[|c]#\n", ":split-line<ret>", "ab#[\n|]#c\n")).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_first_match() -> anyhow::Result<()> {
    test(("#[|f]#oo bar\n", ":s/oo/00/<ret>", "#[f|]#00 bar\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_global_flag() -> anyhow::Result<()> {
    test(("#[|f]#oo boo\n", ":s/o/0/g<ret>", "#[f|]#00 b00\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_vim_magic_group() -> anyhow::Result<()> {
    // `\(fo\|ba\)o` is a group + alternation in vim magic; without translation it
    // would search for the literal text "(fo|ba)o". The harness default preset is
    // spacemacs (vim base), so translation applies and both words are replaced.
    test(("#[|f]#oo bao\n", r":s/\(fo\|ba\)o/X/g<ret>", "#[X|]# X\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_vim_magic_quantifier() -> anyhow::Result<()> {
    // vim `a\+` is one-or-more `a`; a raw Rust pattern reads `a\+` as the literal
    // "a+" (absent here). Translation makes it a quantifier that matches "aaa".
    test(("#[|a]#aa bbb\n", r":s/a\+/X/<ret>", "#[X|]# bbb\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_uppercase_region() -> anyhow::Result<()> {
    // `\U\1` uppercases the whole captured group.
    test(("#[|f]#oo bar\n", r":s/\(foo\)/\U\1/<ret>", "#[F|]#OO bar\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_titlecase_next_char() -> anyhow::Result<()> {
    // `\u\1` uppercases only the first character of the group (title case).
    test(("#[|f]#oo bar\n", r":s/\(foo\)/\u\1/<ret>", "#[F|]#oo bar\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_lowercase_region_until_end() -> anyhow::Result<()> {
    // `\L\1\E \2` lowercases the first group, then `\E` restores normal case for
    // the second group.
    test((
        "#[|F]#OO BAR\n",
        r":s/\(FOO\) \(BAR\)/\L\1\E \2/<ret>",
        "#[f|]#oo BAR\n",
    ))
    .await?;
    Ok(())
}

async fn assert_buffer_after(input: &str, keys: &str, expected: &str) -> anyhow::Result<()> {
    // Use the vim keymap so `:normal`/`:g//normal` replay vim keys (`x`, `dd`, …);
    // the `:sort`/`:m`/`:t`/`:!` commands exercised here are keymap-independent.
    let mut app = helpers::AppBuilder::new()
        .with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        })
        .with_input_text(input)
        .build()?;
    let expected = expected.to_string();
    test_key_sequence(
        &mut app,
        Some(keys),
        Some(&move |app: &zemacs_term::application::Application| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let (_, doc) = zemacs_view::current_ref!(app.editor);
            assert_eq!(doc.text().to_string(), expected);
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn ctrl_a_increments_letter_with_nrformats_alpha() -> anyhow::Result<()> {
    // With `nrformats=alpha`, CTRL-A steps a lone letter a→b (vim).
    test(("#[a|]#bc", ":set nrformats=alpha<ret><C-a>", "#[b|]#bc")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ctrl_a_leaves_letter_untouched_without_alpha() -> anyhow::Result<()> {
    // Without `alpha` in nrformats, CTRL-A on a letter is a no-op. Set an explicit
    // alpha-free value so the thread-local option store can't leak from another test.
    test(("#[a|]#bc", ":set nrformats=hex<ret><C-a>", "#[a|]#bc")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sort_ascending() -> anyhow::Result<()> {
    // vim `:sort` sorts the whole buffer's lines ascending.
    assert_buffer_after("#[|c]#\na\nb\n", ":sort<ret>", "a\nb\nc\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn global_normal_appends_to_matching_lines() -> anyhow::Result<()> {
    // `:g/x/normal A!` appends `!` to every line containing "x".
    assert_buffer_after("#[|a]#x\nbb\ncx\ndd\n", ":g/x/normal A!<ret>", "ax!\nbb\ncx!\ndd\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn global_normal_insert_at_start_of_matching_lines() -> anyhow::Result<()> {
    // `:g/x/normal IZZ` prefixes `ZZ` to each line containing "x".
    assert_buffer_after(
        "#[|a]#x\nbb\ncx\ndd\n",
        ":g/x/normal IZZ<ret>",
        "ZZax\nbb\nZZcx\ndd\n",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn global_normal_dd_deletes_matching_lines() -> anyhow::Result<()> {
    // `:g/x/normal dd` runs the normal-mode operator `dd` on each matching line —
    // bottom-up replay keeps the earlier line indices valid after later removals.
    assert_buffer_after("#[|a]#x\nbb\ncx\ndd\n", ":g/x/normal dd<ret>", "bb\ndd\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn normal_over_range_deletes_last_char_each_line() -> anyhow::Result<()> {
    // `:%normal $x` runs a two-key motion+edit ($ then x) on every line.
    assert_buffer_after("#[|a]#b\ncd\nef\n", ":%normal $x<ret>", "a\nc\ne\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn normal_over_range_appends_to_each_line() -> anyhow::Result<()> {
    // `:%normal A;` appends `;` to every line; the implicit <Esc> returns to Normal
    // between lines.
    assert_buffer_after("#[|a]#\nb\nc\n", ":%normal A;<ret>", "a;\nb;\nc;\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn normal_over_selection_range() -> anyhow::Result<()> {
    // `:2,3normal I#` prefixes `#` to lines 2-3 only.
    assert_buffer_after("#[|a]#\nb\nc\nd\n", ":2,3normal I#<ret>", "a\n#b\n#c\nd\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn move_line_range_to_end() -> anyhow::Result<()> {
    // `:1,2m$` moves lines 1-2 to the end of the buffer.
    assert_buffer_after("#[|a]#\nb\nc\n", ":1,2m$<ret>", "c\na\nb\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn copy_line_range_to_end() -> anyhow::Result<()> {
    // `:1,2t$` copies lines 1-2 to the end, leaving the originals in place.
    assert_buffer_after("#[|a]#\nb\nc\n", ":1,2t$<ret>", "a\nb\nc\na\nb\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sort_whole_file_range() -> anyhow::Result<()> {
    // `:%sort` sorts the whole buffer regardless of the cursor line.
    assert_buffer_after("#[|c]#\na\nb\n", ":%sort<ret>", "a\nb\nc\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sort_line_range() -> anyhow::Result<()> {
    // `:2,3sort` sorts only lines 2-3 (1-based), leaving lines 1 and 4 in place.
    assert_buffer_after("#[|z]#\nc\nb\na\n", ":2,3sort<ret>", "z\nb\nc\na\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sort_arithmetic_range() -> anyhow::Result<()> {
    // Cursor on line 1; `:.,.+2sort` sorts the current line and the next two.
    assert_buffer_after("#[|d]#\nc\nb\na\ne\n", ":.,.+2sort<ret>", "b\nc\nd\na\ne\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sort_pattern_address_range() -> anyhow::Result<()> {
    // `:/foo/,$sort` sorts from the line matching /foo/ (line 2) to the end.
    assert_buffer_after(
        "#[|z]#\nfoo\nd\nc\nb\n",
        ":/foo/,$sort<ret>",
        "z\nb\nc\nd\nfoo\n",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sort_mark_range() -> anyhow::Result<()> {
    // Set mark `a` on line 3 (`jjma`), then `:'a,$sort` sorts from the mark to the
    // end, leaving lines 1-2 in place.
    assert_buffer_after("#[|e]#\nd\nc\nb\na\n", "jjma:'a,$sort<ret>", "e\nd\na\nb\nc\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sort_reverse() -> anyhow::Result<()> {
    // vim `:sort!` reverses the sort.
    assert_buffer_after("#[|a]#\nc\nb\n", ":sort!<ret>", "c\nb\na\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sort_numeric() -> anyhow::Result<()> {
    // vim `:sort n` sorts numerically (10 after 2, not lexicographically before it).
    assert_buffer_after("#[|10]#\n2\n1\n", ":sort n<ret>", "1\n2\n10\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sort_unique() -> anyhow::Result<()> {
    // vim `:sort u` removes duplicate lines.
    assert_buffer_after("#[|b]#\na\nb\n", ":sort u<ret>", "a\nb\n").await
}

#[tokio::test(flavor = "multi_thread")]
async fn filter_whole_file_through_shell() -> anyhow::Result<()> {
    // vim `:%!sort` pipes the whole buffer through `sort` and replaces it.
    let mut app = helpers::AppBuilder::new()
        .with_input_text("#[|c]#\nb\na\n")
        .build()?;
    test_key_sequence(
        &mut app,
        Some(":%!sort<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let (_, doc) = zemacs_view::current_ref!(app.editor);
            assert_eq!(doc.text().to_string(), "a\nb\nc\n");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn filter_current_line_through_shell() -> anyhow::Result<()> {
    // vim `:.!` filters only the current line; the others are untouched.
    let mut app = helpers::AppBuilder::new()
        .with_input_text("abc\n#[|def]#\nghi\n")
        .build()?;
    test_key_sequence(
        &mut app,
        Some(":.!tr a-z A-Z<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let (_, doc) = zemacs_view::current_ref!(app.editor);
            assert_eq!(doc.text().to_string(), "abc\nDEF\nghi\n");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn filter_line_range_through_shell() -> anyhow::Result<()> {
    // `:2,3!sort` filters only lines 2-3 (1-based), leaving the rest untouched.
    let mut app = helpers::AppBuilder::new()
        .with_input_text("#[|d]#\nc\nb\na\n")
        .build()?;
    test_key_sequence(
        &mut app,
        Some(":2,3!sort<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err(), "{:?}", app.editor.get_status());
            let (_, doc) = zemacs_view::current_ref!(app.editor);
            assert_eq!(doc.text().to_string(), "d\nb\nc\na\n");
        }),
        false,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_records_jump() -> anyhow::Result<()> {
    // vim `:s` is a jump command. Move to line 3 (`jj`, no jump), substitute there,
    // move back up (`kk`, no jump), then `<C-o>` returns to the `:s` position on
    // line 3 — proving the substitute recorded a jumplist entry.
    test((
        "#[x|]#xx\nyyy\nzzz\n",
        "jj:s/z/Z/<ret>kk<C-o>",
        "xxx\nyyy\n#[Z|]#zz\n",
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_whole_file() -> anyhow::Result<()> {
    test(("#[|a]#bc\nabc\n", ":%s/b/X/g<ret>", "#[a|]#Xc\naXc\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn replace_word_whole_file() -> anyhow::Result<()> {
    // `:replace-word` rewrites every whole-word occurrence of the word under the
    // cursor across the file; `foozoo` is left alone thanks to the `\b` bounds.
    test((
        "#[|f]#oo bar\nfoozoo\nfoo baz\n",
        ":replace-word qux<ret>",
        "#[q|]#ux bar\nfoozoo\nqux baz\n",
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn replace_word_case_insensitive() -> anyhow::Result<()> {
    // A second `i` arg folds case: `Foo` under the cursor also rewrites `foo`.
    test((
        "#[|F]#oo\nfoo\n",
        ":replace-word bar i<ret>",
        "#[b|]#ar\nbar\n",
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_backreference() -> anyhow::Result<()> {
    // Group + backreference in vim magic syntax: `\(o\+\)` captures "oo", and the
    // replacement `[\1]` inserts it. (In the vim/spacemacs presets the substitute
    // pattern is vim-magic, so a bare `(o+)` would match the literal text "(o+)";
    // the group needs the backslash form `\(…\)`.)
    test((
        "#[|f]#oozoo\n",
        r":s/\(o\+\)/[\1]/<ret>",
        "#[f|]#[oo]zoo\n",
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn global_delete_matching_lines() -> anyhow::Result<()> {
    test((
        "#[|f]#oo\nbar\nfoo baz\nqux\n",
        ":g/foo/d<ret>",
        "#[b|]#ar\nqux\n",
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vglobal_delete_nonmatching_lines() -> anyhow::Result<()> {
    test((
        "#[|f]#oo\nbar\nfoozoo\nqux\n",
        ":v/foo/d<ret>",
        "#[f|]#oo\nfoozoo\n",
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn global_substitute_on_matching_lines() -> anyhow::Result<()> {
    test((
        "#[|f]#oo\nbar\nfoo x\nbaz\n",
        ":g/foo/s/o/0/g<ret>",
        "#[f|]#00\nbar\nf00 x\nbaz\n",
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_delete_inner_word() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("foo #[|b]#ar baz\n", "diw", "foo #[ |]#baz\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_change_inner_paren() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("f(#[|a]#bc)\n", "ci(X", "f(X#[)|]#\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_change_inner_block_alias() -> anyhow::Result<()> {
    // `cib` is vim's alias for `ci(`.
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("f(#[|a]#bc)\n", "cibX", "f(X#[)|]#\n"),
    )
    .await?;
    // `ciB` is vim's alias for `ci{`.
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("f{#[|a]#bc}\n", "ciBX", "f{X#[}|]#\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_delete_find_char() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("ab#[|c]#,de\n", "df,", "ab#[d|]#e\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_change_till_char() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("fo#[|o]#;x\n", "ct;Y", "foY#[;|]#x\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_mark_set_and_jump() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("#[|a]#bc\ndef\nghi\n", "maG`a", "#[a|]#bc\ndef\nghi\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_mark_tracks_edits() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("abc#[|X]#yz\n", "mp0xx`p", "c#[X|]#yz\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_lines_to_bottom() -> anyhow::Result<()> {
    test(("#[|a]#\nb\nc\n", ":m$<ret>", "b\nc\n#[a|]#\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_lines_to_top() -> anyhow::Result<()> {
    test(("a\n#[|b]#\nc\n", ":m0<ret>", "#[b|]#\na\nc\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn copy_lines_to_top() -> anyhow::Result<()> {
    test(("a\n#[|b]#\nc\n", ":t0<ret>", "#[b|]#\na\nb\nc\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_named_register_yank_paste() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // vim-faithful paste: in Normal mode the cursor rests on the pasted text
        // (a bare 1-wide cursor), it does not select the whole pasted region.
        ("#[|x]#\ny\n", "\"ayyj\"ap", "x\ny\n#[x|]#\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_indent_line() -> anyhow::Result<()> {
    test(("#[|a]#bc\ndef\n", ":<gt><ret>", "\t#[|a]#bc\ndef\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_dedent_line() -> anyhow::Result<()> {
    test(("\t#[|a]#bc\ndef\n", ":<lt><ret>", "#[|a]#bc\ndef\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_delete_line() -> anyhow::Result<()> {
    test(("#[|a]#bc\ndef\n", ":d<ret>", "#[d|]#ef\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_yank_put_line() -> anyhow::Result<()> {
    test(("#[|a]#\nb\n", ":y<ret>:put<ret>", "a\n#[a|]#\nb\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_repeat_substitute() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("#[|f]#oo\nfoo\n", ":s/o/0/g<ret>j&", "f00\n#[f|]#00\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_repeat_substitute_global() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("#[|x]#\ny\nx\n", ":s/x/Z/<ret>g&", "#[|Z]#\ny\nZ\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_put_above() -> anyhow::Result<()> {
    test(("#[|a]#\nb\n", ":y<ret>j:put!<ret>", "a\n#[a|]#\nb\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_join_lines() -> anyhow::Result<()> {
    test(("#[|a]#\n  b\nc\n", ":j<ret>", "a#[ |]#b\nc\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_join_lines_nospace() -> anyhow::Result<()> {
    test(("#[|a]#\n  b\nc\n", ":j!<ret>", "a#[b|]#\nc\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_retab_tabs_to_spaces() -> anyhow::Result<()> {
    test(("#[|\t]#ab\n", ":retab<ret>", "#[|    ]#ab\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_left_align() -> anyhow::Result<()> {
    test(("    #[|a]#b\n", ":left 2<ret>", "  #[|a]#b\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_center_align() -> anyhow::Result<()> {
    test(("#[|a]#b\n", ":center 6<ret>", "  #[|a]#b\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_right_align() -> anyhow::Result<()> {
    test(("#[|a]#b\n", ":right 5<ret>", "   #[|a]#b\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ex_undo_redo() -> anyhow::Result<()> {
    // delete a line, :undo restores it, :redo re-deletes.
    test(("#[|a]#\nb\n", ":d<ret>:undo<ret>", "#[|a]#\nb\n")).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_plus_first_nonblank() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("#[|a]#\n  bc\n", "+", "a\n  #[b|]#c\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_macro_record_replay() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // qa: record into a; x: delete char; q: stop; @a: replay -> delete next.
        ("#[|a]#bcde\n", "qaxq@a", "#[c|]#de\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_gv_reselect() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // v + ll selects 3 chars, esc saves, 0 moves to start, gv reselects them.
        ("#[|a]#bcde\n", "vll<esc>0gv", "#[abc|]#de\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_gv_reselect_after_yank() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // vll selects "abc", y yanks it (and, like vim, records the visual area),
        // 0 moves to the start, gv reselects the yanked "abc". Regression: yank
        // used to leave Select without saving the gv selection.
        ("#[|a]#bcde\n", "vlly0gv", "#[abc|]#de\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_gv_reselect_after_register_yank() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // The reported bug: yanking into a named register (`"ay`) must still
        // record the visual area so gv reselects it.
        ("#[|a]#bcde\n", "vll\"ay0gv", "#[abc|]#de\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_gi_insert_at_last() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        ("#[|a]#bc\n", "iX<esc>llgiY<esc>", "XY#[|a]#bc\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_dot_repeat_insert() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // i X <esc> inserts X; l moves right; . repeats the insert.
        ("#[|a]#bc\n", "iX<esc>l.", "XaX#[|b]#c\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_wrap_sexp() -> anyhow::Result<()> {
    use zemacs_core::hashmap;
    use zemacs_term::keymap;
    use zemacs_view::document::Mode;

    // `wrap_sexp` ships on the `SPC k w` leader, but the integration harness cannot
    // drive the space leader (leader sequences don't resolve here — the same reason
    // the split/leader tests use unit tests). Bind the command to a plain key so we
    // still exercise it directly: select "bc", wrap it -> a(bc)d.
    let mut config = zemacs_term::config::Config {
        keys: zemacs_term::keymap::vim::default(),
        ..Default::default()
    };
    config.keys.insert(
        Mode::Normal,
        keymap!({ "Normal mode"
            "w" => wrap_sexp,
        }),
    );
    test_with_config(
        AppBuilder::new().with_config(config),
        ("a#[bc|]#d\n", "w", "a#[(bc)|]#d\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_replace_mode() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // R enters overtype: XY replace a,b; c is untouched.
        ("#[a|]#bc\n", "RXY", "XY#[c|]#\n"),
    )
    .await?;
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // Overtyping past the line end appends (vim behavior).
        ("#[a|]#\n", "RXY", "XY#[\n|]#"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_goto_percent() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // 10 lines; 50% -> line ((50*10+99)/100)=5 (1-based) -> 0-based line 4 ("e").
        (
            "#[a|]#\nb\nc\nd\ne\nf\ng\nh\ni\nj\n",
            "50%",
            "a\nb\nc\nd\n#[e|]#\nf\ng\nh\ni\nj\n",
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sentence_motions() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // ) jumps to the start of the next sentence ("Two").
        ("#[O|]#ne. Two. Three.\n", ")", "One. #[T|]#wo. Three.\n"),
    )
    .await?;
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // ( from inside "Three" jumps back to its start.
        ("One. Two. T#[h|]#ree.\n", "(", "One. Two. #[T|]#hree.\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_sentence_textobject() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // dis deletes the sentence under the cursor ("Two.") leaving the space.
        ("One. T#[w|]#o. Three.\n", "dis", "One. #[ |]#Three.\n"),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn vim_visual_linewise_delete() -> anyhow::Result<()> {
    test_with_config(
        AppBuilder::new().with_config(zemacs_term::config::Config {
            keys: zemacs_term::keymap::vim::default(),
            ..Default::default()
        }),
        // v enters visual mode on line 2; D deletes the whole line linewise.
        ("one\n#[t|]#wo\nthree\n", "vD", "one\n#[t|]#hree\n"),
    )
    .await?;
    Ok(())
}
