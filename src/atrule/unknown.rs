use std::iter::Peekable;

use codemap::{Span, Spanned};

use super::parse::eat_stmts;
use crate::error::SassResult;
use crate::scope::Scope;
use crate::selector::Selector;
use crate::utils::{devour_whitespace, parse_interpolation};
use crate::{RuleSet, Stmt, Token};

#[derive(Debug, Clone)]
pub(crate) struct UnknownAtRule {
    pub name: String,
    pub super_selector: Selector,
    pub params: String,
    pub body: Vec<Spanned<Stmt>>,
}

impl UnknownAtRule {
    pub fn from_tokens<I: Iterator<Item = Token>>(
        toks: &mut Peekable<I>,
        name: &str,
        scope: &mut Scope,
        super_selector: &Selector,
        kind_span: Span,
    ) -> SassResult<UnknownAtRule> {
        let mut params = String::new();
        while let Some(tok) = toks.next() {
            match tok.kind {
                '{' => break,
                '#' => {
                    if toks.peek().unwrap().kind == '{' {
                        toks.next();
                        let interpolation = parse_interpolation(toks, scope, super_selector)?;
                        params.push_str(&interpolation.node.to_css_string(interpolation.span)?);
                        continue;
                    } else {
                        params.push(tok.kind);
                    }
                }
                '\n' | ' ' | '\t' => {
                    devour_whitespace(toks);
                    params.push(' ');
                    continue;
                }
                _ => {}
            }
            params.push(tok.kind);
        }

        let raw_body = eat_stmts(toks, scope, super_selector)?;
        let mut body = Vec::with_capacity(raw_body.len());
        body.push(Spanned {
            node: Stmt::RuleSet(RuleSet::new()),
            span: kind_span,
        });
        let mut rules = Vec::new();
        for stmt in raw_body {
            match stmt.node {
                Stmt::Style(..) => rules.push(stmt),
                _ => body.push(stmt),
            }
        }

        body[0] = Spanned {
            node: Stmt::RuleSet(RuleSet {
                selector: super_selector.clone(),
                rules,
                super_selector: Selector::new(),
            }),
            span: kind_span,
        };

        Ok(UnknownAtRule {
            name: name.to_owned(),
            super_selector: Selector::new(),
            params: params.trim().to_owned(),
            body,
        })
    }
}
