use crate::mcp::protocol::McpError;
use crate::mcp::tool::McpBoxedFuture;
use sea_orm_migration::async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The erased async signature for a prompt builder.
pub type McpPromptFn = fn(Value) -> McpBoxedFuture<Result<Vec<McpPromptMessage>, McpError>>;

/// The erased signature for a prompt's declared arguments.
///
/// A function pointer rather than a `&'static [McpPromptArgument]` because
/// `inventory::submit!` needs a const expression: [`McpPromptArgument`] owns
/// `String`s and so cannot be built in a `static` initializer.
pub type McpPromptArgsFn = fn() -> Vec<McpPromptArgument>;

/// One declared prompt parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
}

impl McpPromptArgument {
    pub fn required(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: Some(description.to_string()),
            required: true,
        }
    }

    pub fn optional(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: Some(description.to_string()),
            required: false,
        }
    }
}

/// A single conversational turn produced by a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptMessage {
    pub role: String,
    pub content: Value,
}

impl McpPromptMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: json!({ "type": "text", "text": text.into() }),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: json!({ "type": "text", "text": text.into() }),
        }
    }
}

/// The reusable prompt-template abstraction.
#[async_trait]
pub trait McpPrompt: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn arguments(&self) -> Vec<McpPromptArgument> {
        Vec::new()
    }

    /// Render the prompt given the caller's supplied arguments.
    async fn render(&self, args: Value) -> Result<Vec<McpPromptMessage>, McpError>;
}

/// A compile-time prompt registration submitted by `#[mcp_prompt]`.
pub struct McpPromptRegistration {
    pub name: &'static str,
    pub description: &'static str,
    pub arguments: McpPromptArgsFn,
    pub required_role: Option<&'static str>,
    pub builder: McpPromptFn,
}

inventory::collect!(McpPromptRegistration);

impl McpPromptRegistration {
    /// The `prompts/list` descriptor for this prompt.
    pub fn to_descriptor(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "arguments": self.declared_arguments(),
        })
    }

    /// The prompt's declared arguments.
    pub fn declared_arguments(&self) -> Vec<McpPromptArgument> {
        (self.arguments)()
    }

    /// Arguments the caller omitted that this prompt declared as required.
    ///
    /// Enforced centrally so an individual prompt does not have to re-check
    /// what `prompts/list` already advertised to the model.
    pub fn missing_required(&self, supplied: &Value) -> Vec<String> {
        self.declared_arguments()
            .iter()
            .filter(|argument| argument.required)
            .filter(|argument| supplied.get(&argument.name).is_none())
            .map(|argument| argument.name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_messages_use_mcp_content_block_shape() {
        let message = McpPromptMessage::user("hello");
        assert_eq!(message.role, "user");
        assert_eq!(message.content["type"], json!("text"));
        assert_eq!(message.content["text"], json!("hello"));
    }

    #[test]
    fn descriptors_include_declared_arguments() {
        fn args() -> Vec<McpPromptArgument> {
            vec![McpPromptArgument::required("topic", "What to write about")]
        }

        let reg = McpPromptRegistration {
            name: "draft_post",
            description: "Draft a blog post",
            arguments: args,
            required_role: None,
            builder: |_args| Box::pin(async { Ok(vec![McpPromptMessage::user("x")]) }),
        };

        let descriptor = reg.to_descriptor();
        assert_eq!(descriptor["name"], json!("draft_post"));
        assert_eq!(descriptor["arguments"][0]["name"], json!("topic"));
        assert_eq!(descriptor["arguments"][0]["required"], json!(true));
    }
}