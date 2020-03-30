//! # grass
//! An implementation of the sass specification in pure rust.
//!
//! All functionality is currently exposed through [`StyleSheet`].
//!
//! Spec progress as of 2020-03-20:
//!
//! | Passing | Failing | Total |
//! |---------|---------|-------|
//! | 1394    | 3699    | 5093  |
//!
//! ## Use as library
//! ```
//! use std::io::{BufWriter, stdout};
//! use grass::{SassResult, StyleSheet};
//!
//! fn main() -> SassResult<()> {
//!     let mut buf = BufWriter::new(stdout());
//!     StyleSheet::from_path("input.scss")?.print_as_css(&mut buf)
//! }
//! ```
//!
//! ## Use as binary
//! ```bash
//! cargo install grass
//! grass input.scss
//! ```

#![warn(
    clippy::all,
    clippy::restriction,
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo
)]
#![deny(missing_debug_implementations)]
#![allow(
    // explicit return makes some things look ugly
    clippy::implicit_return,
    // Self { .. } is less explicit than Foo { .. }
    clippy::use_self,
    // this is way too pedantic -- some things don't need docs!
    clippy::missing_docs_in_private_items,
    // unreachable!() has many valid use cases
    clippy::unreachable,
    // _ => {} has many valid use cases
    clippy::wildcard_enum_match_arm,
    // .expect() has many valid use cases, like when we know a value is `Some(..)`
    clippy::option_expect_used,
    // this is too pedantic -- we are allowed to add numbers!
    clippy::integer_arithmetic,
    // this is too pedantic for now -- the library is changing too quickly for
    // good docs to be written
    clippy::missing_errors_doc,
    // this incorrectly results in errors for types that derive `Debug`
    // https://github.com/rust-lang/rust-clippy/issues/4980
    clippy::let_underscore_must_use,
    // this is too pedantic -- it results in some names being less explicit
    // than they should
    clippy::module_name_repetitions,
    // this is too pedantic -- it is sometimes useful to break up `impl`s
    clippy::multiple_inherent_impl,
    
    // temporarily allowed while under heavy development.
    // eventually these allows should be refactored away
    // to no longer be necessary
    clippy::as_conversions,
    clippy::todo,
    clippy::too_many_lines,
    clippy::panic,
    clippy::option_unwrap_used,
    clippy::result_unwrap_used,
    clippy::result_expect_used,
    clippy::cast_possible_truncation,
    clippy::single_match_else,
    clippy::indexing_slicing,
    clippy::match_same_arms,
    clippy::or_fun_call,
)]
#![cfg_attr(feature = "nightly", feature(track_caller))]
use std::fmt::{self, Display};
use std::fs;
use std::io::Write;
use std::iter::{Iterator, Peekable};
use std::path::Path;

use crate::atrule::{eat_include, AtRule, AtRuleKind, Function, Mixin};
use crate::common::Pos;
use crate::css::Css;
pub use crate::error::{SassError, SassResult};
use crate::format::PrettyPrinter;
use crate::imports::import;
use crate::lexer::Lexer;
use crate::scope::{insert_global_var, Scope, GLOBAL_SCOPE};
use crate::selector::Selector;
use crate::style::Style;
pub(crate) use crate::token::Token;
use crate::utils::{
    devour_whitespace, eat_comment, eat_ident, eat_ident_no_interpolation, eat_variable_value,
    parse_quoted_string, read_until_newline, VariableDecl,
};
use crate::value::Value;

mod args;
mod atrule;
mod builtin;
mod color;
mod common;
mod css;
mod error;
mod format;
mod imports;
mod lexer;
mod scope;
mod selector;
mod style;
mod token;
mod unit;
mod utils;
mod value;

/// Represents a parsed SASS stylesheet with nesting
#[derive(Debug, Clone)]
pub struct StyleSheet(Vec<Stmt>);

#[derive(Clone, Debug)]
pub(crate) enum Stmt {
    /// A [`Style`](/grass/style/struct.Style)
    Style(Box<Style>),
    /// A  [`RuleSet`](/grass/struct.RuleSet.html)
    RuleSet(RuleSet),
    /// A multiline comment: `/* foo bar */`
    MultilineComment(String),
    /// A CSS rule: `@charset "UTF-8";`
    AtRule(AtRule),
}

/// Represents a single rule set. Rule sets can contain other rule sets
///
/// ```scss
/// a {
///   color: blue;
///   b {
///     color: red;
///   }
/// }
/// ```
#[derive(Clone, Debug)]
pub(crate) struct RuleSet {
    selector: Selector,
    rules: Vec<Stmt>,
    // potential optimization: we don't *need* to own the selector
    super_selector: Selector,
}

impl RuleSet {
    pub(crate) const fn new() -> RuleSet {
        RuleSet {
            selector: Selector::new(),
            rules: Vec::new(),
            super_selector: Selector::new(),
        }
    }
}

/// An intermediate representation of what are essentially single lines
/// todo! rename this
#[derive(Clone, Debug)]
enum Expr {
    /// A style: `color: red`
    Style(Box<Style>),
    /// Several styles
    Styles(Vec<Style>),
    /// A full selector `a > h1`
    Selector(Selector),
    /// A variable declaration `$var: 1px`
    VariableDecl(String, Box<Value>),
    /// A mixin declaration `@mixin foo {}`
    MixinDecl(String, Box<Mixin>),
    FunctionDecl(String, Box<Function>),
    /// An include statement `@include foo;`
    Include(Vec<Stmt>),
    /// A multiline comment: `/* foobar */`
    MultilineComment(String),
    Debug(Pos, String),
    Warn(Pos, String),
    AtRule(AtRule),
    // /// Function call: `calc(10vw - 1px)`
    // FuncCall(String, Vec<Token>),
}

/// Print the internal representation of a parsed stylesheet
///
/// Very closely resembles the original SASS, but contains only things translatable
/// to pure CSS: functions, variables, values, and mixins have all been evaluated.
///
/// Use `StyleSheet::print_as_css` to properly convert to CSS.
impl Display for StyleSheet {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        PrettyPrinter::new(f).pretty_print(self).unwrap();
        Ok(())
    }
}

impl StyleSheet {
    #[inline]
    pub fn new(input: &str) -> SassResult<StyleSheet> {
        Ok(StyleSheet(
            StyleSheetParser {
                global_scope: Scope::new(),
                lexer: Lexer::new(input).peekable(),
                rules: Vec::new(),
                scope: 0,
                file: String::from("stdin"),
            }
            .parse_toplevel()?
            .0,
        ))
    }

    #[inline]
    pub fn from_path<P: AsRef<Path> + Into<String>>(p: P) -> SassResult<StyleSheet> {
        Ok(StyleSheet(
            StyleSheetParser {
                global_scope: Scope::new(),
                lexer: Lexer::new(&String::from_utf8(fs::read(p.as_ref())?)?).peekable(),
                rules: Vec::new(),
                scope: 0,
                file: p.into(),
            }
            .parse_toplevel()?
            .0,
        ))
    }

    pub(crate) fn export_from_path<P: AsRef<Path> + Into<String>>(
        p: P,
    ) -> SassResult<(Vec<Stmt>, Scope)> {
        Ok(StyleSheetParser {
            global_scope: Scope::new(),
            lexer: Lexer::new(&String::from_utf8(fs::read(p.as_ref())?)?).peekable(),
            rules: Vec::new(),
            scope: 0,
            file: p.into(),
        }
        .parse_toplevel()?)
    }

    pub(crate) fn from_stmts(s: Vec<Stmt>) -> StyleSheet {
        StyleSheet(s)
    }

    /// Write the internal representation as CSS to `buf`
    ///
    /// ```
    /// use std::io::{BufWriter, stdout};
    /// use grass::{SassResult, StyleSheet};
    ///
    /// fn main() -> SassResult<()> {
    ///     let mut buf = BufWriter::new(stdout());
    ///     StyleSheet::from_path("input.scss")?.print_as_css(&mut buf)
    /// }
    /// ```
    #[inline]
    pub fn print_as_css<W: Write>(self, buf: &mut W) -> SassResult<()> {
        Css::from_stylesheet(self).pretty_print(buf, 0)
    }
}

#[derive(Debug, Clone)]
struct StyleSheetParser<'a> {
    global_scope: Scope,
    lexer: Peekable<Lexer<'a>>,
    rules: Vec<Stmt>,
    scope: u32,
    file: String,
}

impl<'a> StyleSheetParser<'a> {
    fn parse_toplevel(mut self) -> SassResult<(Vec<Stmt>, Scope)> {
        let mut rules: Vec<Stmt> = Vec::new();
        while let Some(Token { kind, .. }) = self.lexer.peek() {
            match kind {
                'a'..='z' | 'A'..='Z' | '_' | '-'
                | '[' | '#' | ':' | '*' | '%' | '.' => rules
                    .extend(self.eat_rules(&Selector::new(), &mut GLOBAL_SCOPE.with(|s| s.borrow().clone()))?),
                &'\t' | &'\n' | ' ' => {
                    self.lexer.next();
                    continue;
                }
                '$' => {
                    self.lexer.next();
                    let name = eat_ident(&mut self.lexer, &Scope::new(), &Selector::new())?;
                    devour_whitespace(&mut self.lexer);
                    if self
                        .lexer
                        .next()
                        .unwrap()
                        .kind
                        != ':'
                    {
                        return Err("expected \":\".".into());
                    }
                    let VariableDecl { val, default, .. } =
                        eat_variable_value(&mut self.lexer, &GLOBAL_SCOPE.with(|s| s.borrow().clone()), &Selector::new())?;
                    GLOBAL_SCOPE.with(|s| {
                        if !default || s.borrow().get_var(&name).is_err() {
                            match s.borrow_mut().insert_var(&name, val) {
                                Ok(..) => Ok(()),
                                Err(e) => Err(e),
                            }
                        } else {
                            Ok(())
                        }
                    })?
                }
                '/' => {
                    self.lexer.next();
                    if '*' == self.lexer.peek().unwrap().kind {
                        self.lexer.next();
                        rules.push(Stmt::MultilineComment(eat_comment(&mut self.lexer, &Scope::new(), &Selector::new())?));
                    } else if '/' == self.lexer.peek().unwrap().kind {
                        read_until_newline(&mut self.lexer);
                        devour_whitespace(&mut self.lexer);
                    } else {
                        todo!()
                    }
                }
                '@' => {
                    self.lexer.next();
                    let at_rule_kind = eat_ident(&mut self.lexer, &Scope::new(), &Selector::new())?;
                    if at_rule_kind.is_empty() {
                        return Err("Expected identifier.".into());
                    }
                    match AtRuleKind::from(at_rule_kind.as_str()) {
                        AtRuleKind::Include => rules.extend(eat_include(
                            &mut self.lexer,
                            &GLOBAL_SCOPE.with(|s| s.borrow().clone()),
                            &Selector::new(),
                        )?),
                        AtRuleKind::Import => {
                            devour_whitespace(&mut self.lexer);
                            let mut file_name = String::new();
                            match self
                                .lexer
                                .next()
                                .unwrap()
                                .kind
                            {
                                q @ '"' | q @ '\'' => {
                                    file_name.push_str(&parse_quoted_string(&mut self.lexer, &Scope::new(), q, &Selector::new())?.unquote().to_string());
                                }
                                _ => todo!("expected ' or \" after @import"),
                            }
                            if self.lexer.next().unwrap().kind != ';' {
                                todo!("no semicolon after @import");
                            }

                            let (new_rules, new_scope) = import(file_name)?;
                            rules.extend(new_rules);
                            GLOBAL_SCOPE.with(|s| {
                                s.borrow_mut().extend(new_scope);
                            });
                        }
                        v => {
                                match AtRule::from_tokens(&v, Pos::new(), &mut self.lexer, &mut GLOBAL_SCOPE.with(|s| s.borrow().clone()), &Selector::new())? {
                                    AtRule::Mixin(name, mixin) => {
                                        GLOBAL_SCOPE.with(|s| {
                                            s.borrow_mut().insert_mixin(&name, *mixin);
                                        });
                                    }
                                    AtRule::Function(name, func) => {
                                        GLOBAL_SCOPE.with(|s| {
                                            s.borrow_mut().insert_fn(&name, *func);
                                        });
                                    }
                                    AtRule::Charset => continue,
                                    AtRule::Error(pos, message) => self.error(pos, &message),
                                    AtRule::Warn(pos, message) => self.warn(pos, &message),
                                    AtRule::Debug(pos, message) => self.debug(pos, &message),
                                    AtRule::Return(_) => {
                                        return Err("This at-rule is not allowed here.".into())
                                    }
                                    AtRule::For(s) => rules.extend(s),
                                    AtRule::Content => return Err("@content is only allowed within mixin declarations.".into()),
                                    AtRule::If(i) => {
                                        rules.extend(i.eval(&mut Scope::new(), &Selector::new())?);
                                    }
                                    u @ AtRule::Unknown(..) => rules.push(Stmt::AtRule(u)),
                                }
                            }
                    }
                },
                '&' => {
                    return Err(
                        "Base-level rules cannot contain the parent-selector-referencing character '&'.".into(),
                    )
                }
                _ => match dbg!(self.lexer.next()) {
                    Some(Token { pos, .. }) => self.error(pos, "unexpected toplevel token"),
                    _ => unsafe { std::hint::unreachable_unchecked() },
                }
            };
        }
        Ok((rules, GLOBAL_SCOPE.with(|s| s.borrow().clone())))
    }

    fn eat_rules(&mut self, super_selector: &Selector, scope: &mut Scope) -> SassResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        while let Some(expr) = eat_expr(&mut self.lexer, scope, super_selector)? {
            match expr {
                Expr::Style(s) => stmts.push(Stmt::Style(s)),
                Expr::AtRule(a) => match a {
                    AtRule::For(s) => stmts.extend(s),
                    AtRule::If(i) => stmts.extend(i.eval(scope, super_selector)?),
                    AtRule::Content => {
                        return Err("@content is only allowed within mixin declarations.".into())
                    }
                    AtRule::Return(..) => return Err("This at-rule is not allowed here.".into()),
                    r => stmts.push(Stmt::AtRule(r)),
                },
                Expr::Styles(s) => stmts.extend(s.into_iter().map(Box::new).map(Stmt::Style)),
                Expr::MixinDecl(name, mixin) => {
                    scope.insert_mixin(&name, *mixin);
                }
                Expr::FunctionDecl(name, func) => {
                    scope.insert_fn(&name, *func);
                }
                Expr::Selector(s) => {
                    self.scope += 1;
                    let rules = self.eat_rules(&super_selector.zip(&s), scope)?;
                    stmts.push(Stmt::RuleSet(RuleSet {
                        super_selector: super_selector.clone(),
                        selector: s,
                        rules,
                    }));
                    self.scope -= 1;
                    if self.scope == 0 {
                        return Ok(stmts);
                    }
                }
                Expr::VariableDecl(name, val) => {
                    if self.scope == 0 {
                        scope.insert_var(&name, *val.clone())?;
                        insert_global_var(&name, *val)?;
                    } else {
                        scope.insert_var(&name, *val)?;
                    }
                }
                Expr::Include(rules) => stmts.extend(rules),
                Expr::Debug(pos, ref message) => self.debug(pos, message),
                Expr::Warn(pos, ref message) => self.warn(pos, message),
                Expr::MultilineComment(s) => stmts.push(Stmt::MultilineComment(s)),
            }
        }
        Ok(stmts)
    }
}

pub(crate) fn eat_expr<I: Iterator<Item = Token>>(
    toks: &mut Peekable<I>,
    scope: &mut Scope,
    super_selector: &Selector,
) -> SassResult<Option<Expr>> {
    let mut values = Vec::with_capacity(5);
    while let Some(tok) = toks.peek() {
        match &tok.kind {
            ':' => {
                let tok = toks.next();
                if devour_whitespace(toks) {
                    let prop = Style::parse_property(
                        &mut values.into_iter().peekable(),
                        scope,
                        super_selector,
                        String::new(),
                    )?;
                    return Ok(Some(Style::from_tokens(toks, scope, super_selector, prop)?));
                } else {
                    values.push(tok.unwrap());
                }
            }
            ';' => {
                toks.next();
                devour_whitespace(toks);
                // special edge case where there was no space between the colon
                // in a style, e.g. `color:red`. todo: refactor
                let mut v = values.into_iter().peekable();
                devour_whitespace(&mut v);
                if v.peek().is_none() {
                    devour_whitespace(toks);
                    return Ok(Some(Expr::Style(Box::new(Style {
                        property: String::new(),
                        value: Value::Null,
                    }))));
                }
                let property = Style::parse_property(&mut v, scope, super_selector, String::new())?;
                let value = Style::parse_value(&mut v, scope, super_selector)?;
                return Ok(Some(Expr::Style(Box::new(Style { property, value }))));
            }
            '}' => {
                if values.is_empty() {
                    toks.next();
                    devour_whitespace(toks);
                    return Ok(None);
                } else {
                    // special edge case where there was no space between the colon
                    // and no semicolon following the style
                    // in a style `color:red`. todo: refactor
                    let mut v = values.into_iter().peekable();
                    let property =
                        Style::parse_property(&mut v, scope, super_selector, String::new())?;
                    let value = Style::parse_value(&mut v, scope, super_selector)?;
                    return Ok(Some(Expr::Style(Box::new(Style { property, value }))));
                }
            }
            '{' => {
                toks.next();
                devour_whitespace(toks);
                return Ok(Some(Expr::Selector(Selector::from_tokens(
                    &mut values.into_iter().peekable(),
                    scope,
                    super_selector,
                )?)));
            }
            '$' => {
                let tok = toks.next().unwrap();
                if toks.peek().unwrap().kind == '=' {
                    values.push(tok);
                    values.push(toks.next().unwrap());
                    continue;
                }
                let name = eat_ident_no_interpolation(toks)?;
                if toks.peek().unwrap().kind == ':' {
                    toks.next();
                    devour_whitespace(toks);
                    let VariableDecl {
                        val,
                        default,
                        global,
                    } = eat_variable_value(toks, scope, super_selector)?;
                    if global {
                        insert_global_var(&name, val.clone())?;
                    }
                    if !default || scope.get_var(&name).is_err() {
                        return Ok(Some(Expr::VariableDecl(name, Box::new(val))));
                    }
                } else {
                    todo!()
                }
            }
            '/' => {
                let tok = toks.next().unwrap();
                let peeked = toks.peek().ok_or("expected more input.")?;
                if peeked.kind == '/' {
                    read_until_newline(toks);
                    devour_whitespace(toks);
                    continue;
                } else if values.is_empty() && peeked.kind == '*' {
                    toks.next();
                    return Ok(Some(Expr::MultilineComment(eat_comment(
                        toks,
                        scope,
                        super_selector,
                    )?)));
                } else {
                    values.push(tok);
                }
            }
            '@' => {
                let pos = toks.next().unwrap().pos();
                match AtRuleKind::from(eat_ident(toks, scope, super_selector)?.as_str()) {
                    AtRuleKind::Include => {
                        devour_whitespace(toks);
                        return Ok(Some(Expr::Include(eat_include(
                            toks,
                            scope,
                            super_selector,
                        )?)));
                    }
                    v => {
                        devour_whitespace(toks);
                        return match AtRule::from_tokens(&v, pos, toks, scope, super_selector)? {
                            AtRule::Mixin(name, mixin) => Ok(Some(Expr::MixinDecl(name, mixin))),
                            AtRule::Function(name, func) => {
                                Ok(Some(Expr::FunctionDecl(name, func)))
                            }
                            AtRule::Charset => todo!("@charset as expr"),
                            AtRule::Debug(a, b) => Ok(Some(Expr::Debug(a, b))),
                            AtRule::Warn(a, b) => Ok(Some(Expr::Warn(a, b))),
                            AtRule::Error(pos, err) => Err(SassError::new(err, pos)),
                            a @ AtRule::Return(_) => Ok(Some(Expr::AtRule(a))),
                            c @ AtRule::Content => Ok(Some(Expr::AtRule(c))),
                            f @ AtRule::If(..) => Ok(Some(Expr::AtRule(f))),
                            f @ AtRule::For(..) => Ok(Some(Expr::AtRule(f))),
                            u @ AtRule::Unknown(..) => Ok(Some(Expr::AtRule(u))),
                        };
                    }
                }
            }
            '#' => {
                values.push(toks.next().unwrap());
                if toks.peek().unwrap().kind == '{' {
                    values.push(toks.next().unwrap());
                    values.extend(eat_interpolation(toks));
                }
            }
            _ => values.push(toks.next().unwrap()),
        };
    }
    Ok(None)
}

fn eat_interpolation<I: Iterator<Item = Token>>(toks: &mut Peekable<I>) -> Vec<Token> {
    let mut vals = Vec::new();
    let mut n = 1;
    for tok in toks {
        match tok.kind {
            '{' => n += 1,
            '}' => n -= 1,
            _ => {}
        }
        vals.push(tok);
        if n == 0 {
            break;
        }
    }
    vals
}

/// Functions that print to stdout or stderr
impl<'a> StyleSheetParser<'a> {
    fn debug(&self, pos: Pos, message: &str) {
        eprintln!("{}:{} Debug: {}", self.file, pos.line(), message);
    }

    fn warn(&self, pos: Pos, message: &str) {
        eprintln!(
            "Warning: {}\n\t{} {}:{} todo!(scope)",
            message,
            self.file,
            pos.line(),
            pos.column()
        );
    }

    fn error(&self, pos: Pos, message: &str) -> ! {
        eprintln!("Error: {}", message);
        eprintln!(
            "{} {}:{} todo!(scope) on line {} at column {}",
            self.file,
            pos.line(),
            pos.column(),
            pos.line(),
            pos.column()
        );
        let padding = vec![' '; format!("{}", pos.line()).len() + 1]
            .iter()
            .collect::<String>();
        eprintln!("{}|", padding);
        eprint!("{} | ", pos.line());
        eprintln!("todo! get line to print as error");
        eprintln!(
            "{}| {}^",
            padding,
            vec![' '; pos.column() as usize].iter().collect::<String>()
        );
        eprintln!("{}|", padding);
        std::process::exit(1);
    }
}
