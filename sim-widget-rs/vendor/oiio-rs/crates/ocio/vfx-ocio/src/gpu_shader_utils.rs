//! The shader text writer (OCIO's `GpuShaderText`: lines and indentation).
//!
//! The per-language spellings OCIO keeps next to it (`GpuShaderUtils.cpp`: vector keywords,
//! texture declarations and samples, ...) live in one place here, `gpu::dialect::Dialect`.

use std::fmt::Write;

// =============================================================================
// Shader Text Builder
// =============================================================================

/// Builder for generating shader code with proper indentation and language-specific syntax.
pub struct ShaderText {
    code: String,
    indent_level: usize,
}

impl ShaderText {
    /// Creates a new shader text builder.
    pub fn new() -> Self {
        Self {
            code: String::new(),
            indent_level: 0,
        }
    }

    /// Returns the generated code.
    pub fn into_code(self) -> String {
        self.code
    }

    /// Returns a reference to the generated code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Increases indent level.
    pub fn indent(&mut self) {
        self.indent_level += 1;
    }

    /// Decreases indent level.
    pub fn dedent(&mut self) {
        if self.indent_level > 0 {
            self.indent_level -= 1;
        }
    }

    /// Writes an empty line.
    pub fn new_line(&mut self) {
        self.code.push('\n');
    }

    /// Writes a line with proper indentation.
    pub fn line(&mut self, text: &str) {
        for _ in 0..self.indent_level {
            self.code.push_str("    ");
        }
        self.code.push_str(text);
        self.code.push('\n');
    }

    /// Writes formatted text with proper indentation.
    pub fn linef(&mut self, text: impl std::fmt::Display) {
        for _ in 0..self.indent_level {
            self.code.push_str("    ");
        }
        writeln!(self.code, "{}", text).unwrap();
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_indented_by_level() {
        let mut st = ShaderText::new();
        st.line("a {");
        st.indent();
        st.linef(format!("b = {};", 1));
        st.dedent();
        st.line("}");
        assert_eq!(st.code(), "a {\n    b = 1;\n}\n");
    }
}
