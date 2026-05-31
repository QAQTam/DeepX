fn main() {
    let content = r#"<tool_calls>
<invoke name="read_file">
<parameter name="end_line" string="false">450</parameter>
<parameter name="path" string="true">D:\project\DeepX\crates\dsx-agent\src\runner\turn.rs</parameter>
<parameter name="start_line" string="false">300</parameter>
</invoke>
</tool_calls>"#;

    // Simulate the turn.rs detection
    let has_invoke = content.contains("<invoke ");
    let has_tool_calls = content.contains("<tool_calls>");

    println!("has_invoke: {}", has_invoke);
    println!("has_tool_calls: {}", has_tool_calls);

    // Simulate parse_xml_tool_calls
    let tool_names: Vec<String> = vec![
        "read_file".into(), "write_file".into(), "exec".into(), "explore".into()
    ];

    let (cleaned, tcs) = dsx_agent::tool_parser::parse_xml_tool_calls(content, &tool_names);
    println!("cleaned: {:?}", cleaned);
    println!("tcs count: {}", tcs.len());
    for tc in &tcs {
        println!("  name={}, args={}", tc.function.name, tc.function.arguments);
    }
}
