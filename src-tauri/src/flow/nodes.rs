use serde::{Deserialize, Serialize};
use serde_json::Value;


/// Node execution context passed between nodes in a flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeContext {
    pub variables: std::collections::HashMap<String, Value>,
    pub last_output: Option<Value>,
}

impl NodeContext {
    pub fn new() -> Self {
        Self {
            variables: std::collections::HashMap::new(),
            last_output: None,
        }
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.variables.insert(key.to_string(), value);
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.variables.get(key)
    }
}

impl Default for NodeContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the prompt template from node data, substituting variables from context
pub fn render_template(template: &str, ctx: &NodeContext) -> String {
    let mut result = template.to_string();
    for (key, value) in &ctx.variables {
        let placeholder = format!("{{{{{}}}}}", key);
        let val_str = match value {
            Value::String(s) => s.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        };
        result = result.replace(&placeholder, &val_str);
    }
    result
}

/// Evaluate a simple condition string against the context
/// Supports: variable == "value", variable != "value", variable contains "text"
pub fn evaluate_condition(condition: &str, ctx: &NodeContext) -> bool {
    let condition = condition.trim();

    if let Some((left, right)) = condition.split_once(" == ") {
        let left_val = resolve_value(left.trim(), ctx);
        let right_val = right.trim().trim_matches('"');
        return left_val.as_deref() == Some(right_val);
    }

    if let Some((left, right)) = condition.split_once(" != ") {
        let left_val = resolve_value(left.trim(), ctx);
        let right_val = right.trim().trim_matches('"');
        return left_val.as_deref() != Some(right_val);
    }

    if let Some((left, right)) = condition.split_once(" contains ") {
        let left_val = resolve_value(left.trim(), ctx).unwrap_or_default();
        let right_val = right.trim().trim_matches('"');
        return left_val.contains(right_val);
    }

    // Default to true for empty or unparseable conditions
    condition.is_empty()
}

fn resolve_value(expr: &str, ctx: &NodeContext) -> Option<String> {
    if expr.starts_with('"') && expr.ends_with('"') {
        return Some(expr.trim_matches('"').to_string());
    }

    ctx.get(expr).map(|v| match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    })
}
