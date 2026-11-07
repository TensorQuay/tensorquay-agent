//! PROMPT-U-014: on a native-tools route, the text that streams while a reply is still arriving stops before anything
//! that could begin a call written as text (the forms that count on such a route: a reply that is one JSON object, a
//! `tqa_call` or `tool_call` block, the native markup). The rest waits for the whole reply, so a call's markup is never
//! shown; everything else streams as it comes.
use tqa_prompt::explicit_streamable_len;

#[test]
fn prompt_u_014_prose_and_other_code_stream_as_they_come() {
    for text in [
        "Photosynthesis turns light into sugar.",
        "If a < b and b > c, then\n",
        "Use x<y or <b>bold</b> or <tool> or <functional>.",
        "Here it is:\n```python\nprint(1)\n```\nDone.",
        "An answer {with braces}",
        "Tools: ```tool_call is not at a line start",
    ] {
        assert_eq!(explicit_streamable_len(text), text.len(), "{text:?}");
    }
}

#[test]
fn prompt_u_014_a_possible_call_is_held_from_where_it_could_start() {
    for (text, shown) in [
        // A reply that may be one JSON object: all of it, until the end.
        ("{\"name\": \"write_file\"", ""),
        ("  \n{", ""),
        ("", ""),
        ("   ", ""),
        // A tqa_call or tool_call block, and a fence whose label could still become one.
        ("Writing it.\n```tool_call\n{}", "Writing it.\n"),
        ("Writing it.\n```tqa_call\n", "Writing it.\n"),
        ("Writing it.\n```tool_", "Writing it.\n"),
        // A json fence may be a call when it ends the reply (F25, PROMPT-U-016), so it waits for the end too.
        (
            "Here it is:\n```json\n{\"name\": \"write_file\"}\n```\n",
            "Here it is:\n",
        ),
        ("Here it is:\n```js", "Here it is:\n"),
        ("Writing it.\n```", "Writing it.\n"),
        ("Writing it.\n``", "Writing it.\n"),
        ("Writing it.\n`", "Writing it.\n"),
        // The native markup, complete or still arriving.
        ("I'll write it.\n\n<function-call>\n{", "I'll write it.\n\n"),
        ("I'll write it.\n<function_call>", "I'll write it.\n"),
        ("I'll write it.\n<tool_call>{", "I'll write it.\n"),
        ("I'll write it. <function=write_file>", "I'll write it. "),
        ("I'll write it. <parameter=path>", "I'll write it. "),
        ("I'll write it. </tool_call>", "I'll write it. "),
        ("I'll write it. <tool", "I'll write it. "),
        ("I'll write it. <", "I'll write it. "),
        ("I'll write it. <function", "I'll write it. "),
    ] {
        assert_eq!(&text[..explicit_streamable_len(text)], shown, "{text:?}");
    }
}

#[test]
fn prompt_u_014_a_fence_that_isnt_a_call_no_longer_holds_once_its_label_is_known() {
    // A python block is shown; a later tool_call block is still held.
    let text = "First:\n```python\nx = 1\n```\nThen:\n```tool_call\n{";
    assert_eq!(
        &text[..explicit_streamable_len(text)],
        "First:\n```python\nx = 1\n```\nThen:\n"
    );
}
