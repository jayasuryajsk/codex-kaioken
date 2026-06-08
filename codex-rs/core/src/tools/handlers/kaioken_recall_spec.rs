use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub const KAIOKEN_RECALL_TOOL_NAME: &str = "kaioken_recall";

pub fn create_kaioken_recall_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "query".to_string(),
            JsonSchema::string(Some(
                "Natural-language behavior question, exact symbol, path, or error text to locate."
                    .to_string(),
            )),
        ),
        (
            "intent".to_string(),
            JsonSchema::string(Some(
                "Optional routing hint: auto, exact, symbol, behavior, or architecture."
                    .to_string(),
            )),
        ),
        (
            "budget".to_string(),
            JsonSchema::string(Some(
                "Optional retrieval budget hint: fast, hybrid, or deep."
                    .to_string(),
            )),
        ),
        (
            "path".to_string(),
            JsonSchema::string(Some(
                "Optional path to search. Relative paths resolve against the current working directory."
                    .to_string(),
            )),
        ),
        (
            "limit".to_string(),
            JsonSchema::number(Some(
                "Maximum evidence rows to return. Defaults to 8 and is capped at 20.".to_string(),
            )),
        ),
        (
            "include_tests".to_string(),
            JsonSchema::boolean(Some(
                "Include tests, docs, and generated files in retrieval. Defaults to false."
                    .to_string(),
            )),
        ),
        (
            "glob".to_string(),
            JsonSchema::string(Some(
                "Optional source glob for semantic fallback, for example src/**/*.ts.".to_string(),
            )),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: KAIOKEN_RECALL_TOOL_NAME.to_string(),
        description: "Default Kaioken code retrieval tool. Use this before broad rg/read exploration. It routes exact symbol/error lookups through a fast path, behavior questions through indexed lexical/path ranking, and architecture questions through deeper retrieval only when useful. Returns compact evidence files, snippets, strategy, match counts, and notes.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["query".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}
