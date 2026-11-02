//! The model that turns pasted text into a creature file.
//!
//! It is asked once, when a creature is drafted, and what it writes is a
//! file like any other: checked by the same loader, shown to be reviewed,
//! and saved to disk. Nothing about a fight ever asks it again - see
//! `DESIGN.md`, "the agent is a compiler, not a runtime".

use std::io::Write;
use std::process::{Command, Stdio};

/// Something that answers a prompt with text.
pub trait Agent: Send + Sync {
    /// `system` is what it is and how to answer; `prompt` is the request.
    fn ask(&self, system: &str, prompt: &str) -> Result<String, String>;
}

/// The Claude Code command line, run once per question with no tools: it
/// answers from the prompt alone, signed in however the machine already is.
///
/// `CRUCIBLE_AGENT` names the program if it is not `claude` on the path,
/// and `CRUCIBLE_AGENT_MODEL` the model if not `sonnet`.
pub struct Cli {
    program: String,
    model: String,
}

impl Cli {
    pub fn from_env() -> Self {
        let or = |name: &str, default: &str| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| default.to_string())
        };
        Self {
            program: or("CRUCIBLE_AGENT", "claude"),
            model: or("CRUCIBLE_AGENT_MODEL", "sonnet"),
        }
    }
}

impl Agent for Cli {
    fn ask(&self, system: &str, prompt: &str) -> Result<String, String> {
        // Its instructions go in a file: they are longer than a command
        // line is allowed to be on some systems. And it runs somewhere
        // empty, so nothing of this repo is read as part of the question.
        let folder = std::env::temp_dir().join(format!("crucible-agent-{}", std::process::id()));
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let instructions = folder.join("system.txt");
        std::fs::write(&instructions, system).map_err(|e| e.to_string())?;

        let mut child = Command::new(&self.program)
            .arg("--print")
            .arg("--no-session-persistence")
            .arg("--strict-mcp-config")
            .args(["--tools", ""])
            .args(["--model", &self.model])
            .arg("--system-prompt-file")
            .arg(&instructions)
            .current_dir(&folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!(
                    "could not start `{}` ({e}) - drafting with AI needs Claude Code installed \
                     and signed in, or CRUCIBLE_AGENT set to a program that answers a prompt",
                    self.program
                )
            })?;
        child
            .stdin
            .take()
            .ok_or("the model takes no input")?
            .write_all(prompt.as_bytes())
            .map_err(|e| e.to_string())?;
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        let said = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !output.status.success() || said.is_empty() {
            let why = String::from_utf8_lossy(&output.stderr);
            let why = if why.trim().is_empty() {
                &said
            } else {
                why.trim()
            };
            return Err(format!("the model did not answer: {why}"));
        }
        Ok(said)
    }
}
