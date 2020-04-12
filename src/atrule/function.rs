use std::iter::Peekable;

use super::eat_stmts;

use codemap::{Span, Spanned};

use crate::args::{eat_func_args, CallArgs, FuncArgs};
use crate::atrule::AtRule;
use crate::error::SassResult;
use crate::scope::Scope;
use crate::selector::Selector;
use crate::utils::{devour_whitespace, eat_ident};
use crate::value::Value;
use crate::{Stmt, Token};

#[derive(Debug, Clone)]
pub(crate) struct Function {
    scope: Scope,
    args: FuncArgs,
    body: Vec<Spanned<Stmt>>,
    pos: Span,
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        self.pos == other.pos
    }
}

impl Eq for Function {}

impl Function {
    pub fn new(scope: Scope, args: FuncArgs, body: Vec<Spanned<Stmt>>, pos: Span) -> Self {
        Function {
            scope,
            args,
            body,
            pos,
        }
    }

    pub fn decl_from_tokens<I: Iterator<Item = Token>>(
        toks: &mut Peekable<I>,
        scope: Scope,
        super_selector: &Selector,
    ) -> SassResult<(String, Function)> {
        let Spanned { node: name, span } = eat_ident(toks, &scope, super_selector)?;
        devour_whitespace(toks);
        let args = match toks.next() {
            Some(Token { kind: '(', .. }) => eat_func_args(toks, &scope, super_selector)?,
            Some(Token { pos, .. }) => return Err(("expected \"(\".", pos).into()),
            None => return Err(("expected \"(\".", span).into()),
        };

        devour_whitespace(toks);

        let body = eat_stmts(toks, &mut scope.clone(), super_selector)?;
        devour_whitespace(toks);

        Ok((name, Function::new(scope, args, body, span)))
    }

    pub fn args(
        mut self,
        mut args: CallArgs,
        scope: &Scope,
        super_selector: &Selector,
    ) -> SassResult<Function> {
        for (idx, arg) in self.args.0.iter().enumerate() {
            if arg.is_variadic {
                let span = args.span();
                self.scope.insert_var(
                    &arg.name,
                    Spanned {
                        node: Value::ArgList(args.get_variadic(scope, super_selector)?),
                        span,
                    },
                )?;
                break;
            }
            let val = match args.get_positional(idx, scope, super_selector) {
                Some(v) => v?,
                None => match args.get_named(arg.name.clone(), scope, super_selector) {
                    Some(v) => v?,
                    None => match &arg.default {
                        Some(v) => Value::from_tokens(
                            &mut v.iter().cloned().peekable(),
                            scope,
                            super_selector,
                        )?,
                        None => {
                            return Err(
                                (format!("Missing argument ${}.", &arg.name), args.span()).into()
                            )
                        }
                    },
                },
            };
            self.scope.insert_var(&arg.name, val)?;
        }
        Ok(self)
    }

    pub fn body(&self) -> Vec<Spanned<Stmt>> {
        self.body.clone()
    }

    pub fn call(&self, super_selector: &Selector, stmts: Vec<Spanned<Stmt>>) -> SassResult<Value> {
        for stmt in stmts {
            match stmt.node {
                Stmt::AtRule(AtRule::Return(toks)) => {
                    return Ok(Value::from_tokens(
                        &mut toks.into_iter().peekable(),
                        &self.scope,
                        super_selector,
                    )?
                    .node)
                }
                Stmt::AtRule(AtRule::For(..)) => todo!("@for in function"),
                Stmt::AtRule(AtRule::If(i)) => {
                    if let Ok(v) = self.call(
                        super_selector,
                        i.eval(&mut self.scope.clone(), super_selector)?,
                    ) {
                        return Ok(v);
                    }
                }
                _ => return Err(("This at-rule is not allowed here.", stmt.span).into()),
            }
        }
        Err(("Function finished without @return.", self.pos).into())
    }
}
