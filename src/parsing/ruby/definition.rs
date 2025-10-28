//! Ruby language definition for the registry
//!
//! Provides the Ruby language implementation that self-registers
//! with the global registry. This module defines how Ruby parsers
//! and behaviors are created based on settings.

use std::sync::Arc;

use super::{RubyBehavior, RubyParser};
use crate::parsing::{LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser};
use crate::{IndexResult, Settings};

/// Ruby language definition
pub struct RubyLanguage;

impl RubyLanguage {
    /// Language identifier constant
    pub const ID: LanguageId = LanguageId::new("ruby");
}

impl LanguageDefinition for RubyLanguage {
    fn id(&self) -> LanguageId {
        Self::ID
    }

    fn name(&self) -> &'static str {
        "Ruby"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rb", "rake", "gemspec"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = RubyParser::new().map_err(|e| crate::IndexError::General(e.to_string()))?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(RubyBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or_else(|| self.default_enabled())
    }
}

/// Register Ruby language with the global registry
///
/// This function is called from initialize_registry() to add
/// Ruby support to the system.
pub(crate) fn register(registry: &mut crate::parsing::LanguageRegistry) {
    registry.register(Arc::new(RubyLanguage));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ruby_definition() {
        let ruby = RubyLanguage;

        assert_eq!(ruby.id(), LanguageId::new("ruby"));
        assert_eq!(ruby.name(), "Ruby");
        assert_eq!(ruby.extensions(), &["rb", "rake", "gemspec"]);
    }

    #[test]
    fn test_ruby_enabled_by_default() {
        let ruby = RubyLanguage;
        let settings = Settings::default();

        // Should be enabled by default
        assert!(ruby.is_enabled(&settings));
    }

    #[test]
    fn test_ruby_can_be_enabled() {
        let ruby = RubyLanguage;

        // Create settings with Ruby enabled
        let mut settings = Settings::default();
        let mut lang_config = std::collections::HashMap::new();
        let mut ruby_config = crate::config::LanguageConfig::default();
        ruby_config.enabled = true;
        lang_config.insert("ruby".to_string(), ruby_config);
        settings.languages = lang_config;

        assert!(ruby.is_enabled(&settings));
    }
}
