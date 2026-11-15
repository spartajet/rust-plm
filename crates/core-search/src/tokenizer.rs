//! JiebaTokenizer（增量20 RPT-03，ADR-0005）：jieba 分词 + CJK 双连字（bigram）兜底。
//! 纯 jieba 对「承座」这类部件名子串查询召回不足（词典未收录切不出该词），
//! 叠加相邻汉字 2-gram 保证子串命中；拉丁词小写化。查询侧同分析器——分词语义对齐。

use jieba_rs::{Jieba, TokenizeMode};
use std::sync::Arc;
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};

#[derive(Clone)]
pub struct JiebaTokenizer {
    jieba: Arc<Jieba>,
}

impl Default for JiebaTokenizer {
    fn default() -> Self {
        Self {
            jieba: Arc::new(Jieba::new()),
        }
    }
}

impl JiebaTokenizer {
    pub const NAME: &str = "jieba_bigram";
}

impl Tokenizer for JiebaTokenizer {
    type TokenStream<'a> = JiebaTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let mut tokens: Vec<Token> = Vec::new();
        let mut push = |start: usize, end: usize, raw: &str, pos: &mut usize| {
            if raw.chars().all(char::is_whitespace) {
                return;
            }
            tokens.push(Token {
                offset_from: start,
                offset_to: end,
                position: *pos,
                text: raw.to_lowercase(),
                position_length: 1,
            });
            *pos += 1;
        };
        // ① jieba 分词（Search 模式粒度：粗细兼顾；tokenize 带字节偏移）
        let mut position = 0usize;
        for tkn in self.jieba.tokenize(text, TokenizeMode::Search, true) {
            push(tkn.start, tkn.end, tkn.word, &mut position);
        }
        // ② CJK 相邻双字兜底（保证「承座」等子串召回；与 jieba 词重复无害）
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        for w in chars.windows(2) {
            let (s0, c0) = w[0];
            let (s1, c1) = w[1];
            if is_cjk(c0) && is_cjk(c1) {
                let end = s1 + c1.len_utf8();
                push(s0, end, &text[s0..end], &mut position);
            }
        }
        JiebaTokenStream {
            tokens: tokens.into_iter(),
            current: None,
        }
    }
}

fn is_cjk(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
}

pub struct JiebaTokenStream {
    tokens: std::vec::IntoIter<Token>,
    current: Option<Token>,
}

impl TokenStream for JiebaTokenStream {
    fn advance(&mut self) -> bool {
        match self.tokens.next() {
            Some(t) => {
                self.current = Some(t);
                true
            }
            None => {
                self.current = None;
                false
            }
        }
    }

    fn token(&self) -> &Token {
        self.current
            .as_ref()
            .expect("token() 调用于 advance 之前或流结束")
    }

    fn token_mut(&mut self) -> &mut Token {
        self.current
            .as_mut()
            .expect("token_mut() 调用于 advance 之前或流结束")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::tokenizer::TextAnalyzer;

    fn analyze(analyzer: &mut TextAnalyzer, text: &str) -> Vec<String> {
        let mut ts = analyzer.token_stream(text);
        let mut out = Vec::new();
        while ts.advance() {
            out.push(ts.token().text.clone());
        }
        out
    }

    #[test]
    fn jieba_and_bigram_union() {
        let mut a = TextAnalyzer::builder(JiebaTokenizer::default()).build();
        let t = analyze(&mut a, "轴承座");
        assert!(t.contains(&"轴承".to_string()), "jieba 词级: {t:?}");
        assert!(t.contains(&"承座".to_string()), "bigram 子串兜底: {t:?}");
        assert!(
            t.contains(&"轴承座".to_string()),
            "cut_for_search 粗粒度: {t:?}"
        );
    }

    #[test]
    fn latin_lowercased() {
        let mut a = TextAnalyzer::builder(JiebaTokenizer::default()).build();
        let t = analyze(&mut a, "Bearing ABC");
        assert!(t.contains(&"bearing".to_string()));
        assert!(t.contains(&"abc".to_string()));
        assert!(!t.contains(&"ABC".to_string()));
    }

    #[test]
    fn pure_ascii_alnum_words() {
        let mut a = TextAnalyzer::builder(JiebaTokenizer::default()).build();
        let t = analyze(&mut a, "Q235 支架");
        assert!(t.contains(&"q235".to_string()), "{t:?}");
        assert!(t.contains(&"支架".to_string()));
    }

    #[test]
    fn whitespace_only_yields_no_tokens() {
        let mut a = TextAnalyzer::builder(JiebaTokenizer::default()).build();
        assert!(analyze(&mut a, "   ").is_empty());
    }
}
