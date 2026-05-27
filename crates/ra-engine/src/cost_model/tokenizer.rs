//! Tokenizer for converting Lime parse tokens to neural model token IDs.
//!
//! The tokenizer maps SQL syntax tokens to integer IDs for the transformer model.
//! It also encodes latency budget constraints as special tokens to provide
//! optimization context to the model.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Time budget for query optimization.
///
/// Encoded as special tokens to give the model context about how much time
/// it has to explore the search space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum TimeBudget {
    /// < 1ms - Aggressive pruning, skip expensive rewrites
    UltraFast,
    /// 1-10ms - Moderate exploration
    Fast,
    /// 10-100ms - Standard optimization
    #[default]
    Balanced,
    /// > 100ms - Full e-graph search
    Exhaustive,
}


/// Tokenizer configuration loaded from tokenizer.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenizerConfig {
    pub version: String,
    pub vocab_size: usize,
    pub special_tokens: SpecialTokens,
    pub vocab: HashMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialTokens {
    pub padding: u32,
    pub unknown: u32,
    pub budget_ultra_fast: u32,
    pub budget_fast: u32,
    pub budget_balanced: u32,
    pub budget_exhaustive: u32,
}

/// Tokenizer for SQL queries.
///
/// Converts Lime parse tokens + budget context into token IDs for the model.
pub struct Tokenizer {
    config: TokenizerConfig,
}

impl Tokenizer {
    /// Load tokenizer from JSON configuration file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or the JSON is malformed.
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path)?;
        let config: TokenizerConfig = serde_json::from_str(&contents)?;
        Ok(Self { config })
    }

    /// Encode a time budget as a special token.
    #[must_use] 
    pub fn encode_budget(&self, budget: TimeBudget) -> u32 {
        match budget {
            TimeBudget::UltraFast => self.config.special_tokens.budget_ultra_fast,
            TimeBudget::Fast => self.config.special_tokens.budget_fast,
            TimeBudget::Balanced => self.config.special_tokens.budget_balanced,
            TimeBudget::Exhaustive => self.config.special_tokens.budget_exhaustive,
        }
    }

    /// Encode SQL text into token IDs.
    ///
    /// This is a simplified version that tokenizes based on keywords.
    /// In production, this would use the Lime parser's token stream.
    #[must_use] 
    pub fn encode(&self, sql: &str, budget: TimeBudget) -> Vec<u32> {
        let mut tokens = Vec::new();

        // Add budget token first (provides context)
        tokens.push(self.encode_budget(budget));

        // Simple keyword-based tokenization
        // In production, use Lime parser token stream
        for word in sql.split_whitespace() {
            let token_str = word.to_uppercase();
            let token_id = self.config.vocab
                .get(&token_str)
                .copied()
                .unwrap_or(self.config.special_tokens.unknown);
            tokens.push(token_id);
        }

        tokens
    }

    /// Pad or truncate token sequence to fixed length.
    pub fn pad(&self, tokens: &mut Vec<u32>, max_len: usize) {
        if tokens.len() < max_len {
            tokens.resize(max_len, self.config.special_tokens.padding);
        } else if tokens.len() > max_len {
            tokens.truncate(max_len);
        }
    }

    /// Get vocabulary size.
    #[must_use] 
    pub fn vocab_size(&self) -> usize {
        self.config.vocab_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_budget() {
        let config = TokenizerConfig {
            version: "1.0".to_string(),
            vocab_size: 512,
            special_tokens: SpecialTokens {
                padding: 0,
                unknown: 1,
                budget_ultra_fast: 2,
                budget_fast: 3,
                budget_balanced: 4,
                budget_exhaustive: 5,
            },
            vocab: HashMap::new(),
        };

        let tokenizer = Tokenizer { config };

        assert_eq!(tokenizer.encode_budget(TimeBudget::UltraFast), 2);
        assert_eq!(tokenizer.encode_budget(TimeBudget::Fast), 3);
        assert_eq!(tokenizer.encode_budget(TimeBudget::Balanced), 4);
        assert_eq!(tokenizer.encode_budget(TimeBudget::Exhaustive), 5);
    }

    #[test]
    fn test_pad() {
        let config = TokenizerConfig {
            version: "1.0".to_string(),
            vocab_size: 512,
            special_tokens: SpecialTokens {
                padding: 0,
                unknown: 1,
                budget_ultra_fast: 2,
                budget_fast: 3,
                budget_balanced: 4,
                budget_exhaustive: 5,
            },
            vocab: HashMap::new(),
        };

        let tokenizer = Tokenizer { config };

        // Test padding
        let mut tokens = vec![10, 20, 30];
        tokenizer.pad(&mut tokens, 5);
        assert_eq!(tokens, vec![10, 20, 30, 0, 0]);

        // Test truncation
        let mut tokens = vec![10, 20, 30, 40, 50, 60];
        tokenizer.pad(&mut tokens, 4);
        assert_eq!(tokens, vec![10, 20, 30, 40]);
    }
}
