//! what the live view shows of a tool call: the file it touches and the text it
//! will write, read through the same path check the tool itself uses.

use serde_json::Value;

/// Describes a file tool call for the live view: the path, the text it shows and
/// whether it writes. A read goes through the same path check as the tool, so a
/// sandboxed read is never shown. Hands back None for any other tool.
pub(super) fn file_event(
    name: &str,
    args: &Value,
    ctx: &crate::tools::Ctx,
) -> Option<(String, String, bool)> {
    match name {
        "write_file" => Some((
            args.get("path")?.as_str()?.to_string(),
            args.get("content")?.as_str()?.to_string(),
            true,
        )),
        // a read shows the file as it is now, before the tool runs, through the
        // same path check as the tool: a sandboxed read is never shown either
        "read_file" => {
            let raw = args.get("path")?.as_str()?;
            let path = crate::tools::resolve(ctx, raw).ok()?;
            let text = std::fs::read_to_string(&path).ok()?;
            Some((raw.to_string(), text, false))
        }
        _ => None,
    }
}
