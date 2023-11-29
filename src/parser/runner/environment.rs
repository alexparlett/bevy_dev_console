use std::{cell::RefCell, rc::Rc};

use ahash::AHashMap;
use logos::Span;

use super::{eval_expression, stdlib, EvalParams, RunError, Value};
use crate::parser::{parser::Expression, Spanned};

/// Get around implementation of Result causing stupid errors
pub(super) struct ResultContainer<T, E>(pub Result<T, E>);

impl<T, E> From<ResultContainer<T, E>> for Result<T, E> {
    fn from(ResultContainer(result): ResultContainer<T, E>) -> Self {
        result
    }
}
impl<T: Into<Value>, E> From<Result<T, E>> for ResultContainer<Value, E> {
    fn from(result: Result<T, E>) -> Self {
        ResultContainer(result.map(|v| v.into()))
    }
}

pub struct Function {
    pub argument_count: usize,
    pub body: Box<dyn Fn(Vec<Spanned<Expression>>, EvalParams) -> Result<Value, RunError>>,
}

/// Trait that represents a [`Fn`] that can be turned into a [`Function`].
pub trait IntoFunction<T> {
    fn into_function(self) -> Function;
}

macro_rules! replace_expr {
    ($_t:ident $sub:expr) => {
        $sub
    };
}

macro_rules! count_idents {
    ($($tts:ident)*) => {0usize $(+ replace_expr!($tts 1usize))*};
}

macro_rules! impl_into_function {
    (
        $($(
                $params:ident
        ),+)?
    ) => {
        impl<F $(, $($params: TryFrom<Spanned<Value>>),+ )?, R> IntoFunction<( $($($params,)+)? )> for F
        where
            F: Fn($($($params),+)?) -> R + 'static,
            R: Into<ResultContainer<Value, RunError>>,
        {
            fn into_function(self) -> Function {
                let body = Box::new(move |args: Vec<Spanned<Expression>>, params: EvalParams| {
                    let EvalParams {
                        world,
                        environment,
                        registrations,
                    } = params;
                    #[allow(unused_variables, unused_mut)]
                    let mut args = args.into_iter().map(move |expr| {
                        Spanned {
                            span: expr.span.clone(),
                            value: eval_expression(
                                expr,
                                EvalParams {
                                    world,
                                    environment,
                                    registrations,
                                }
                            )
                        }
                    });
                    self(
                        $($({
                            let _: $params; // Tell rust im talking abouts the $params
                            let arg = args.next()
                            .unwrap();
                            Spanned {
                                span: arg.span,
                                value: arg.value?
                            }
                            .try_into()
                            .unwrap_or_else(|_| unreachable!())
                        }),+)?
                    )
                    .into().into()
                });
                let argument_count = count_idents!($($($params)+)?);

                Function { body, argument_count }
            }
        }
    }
}

impl_into_function!();
impl_into_function!(T1);
impl_into_function!(T1, T2);
impl_into_function!(T1, T2, T3);
impl_into_function!(T1, T2, T3, T4);
impl_into_function!(T1, T2, T3, T4, T5);
impl_into_function!(T1, T2, T3, T4, T5, T6);
impl_into_function!(T1, T2, T3, T4, T5, T6, T7);
impl_into_function!(T1, T2, T3, T4, T5, T6, T7, T8);

pub enum Variable {
    Unmoved(Rc<RefCell<Value>>),
    Moved,
    Function(Function),
}

/// The environment stores all variables and functions.
pub struct Environment {
    parent: Option<Box<Environment>>,
    variables: AHashMap<String, Variable>,
}

impl Default for Environment {
    fn default() -> Self {
        let mut env = Self {
            parent: None,
            variables: AHashMap::new(),
        };

        stdlib::register(&mut env);

        env
    }
}

impl Environment {
    pub fn set(&mut self, name: String, value: Rc<RefCell<Value>>) -> Result<(), RunError> {
        self.variables.insert(name, Variable::Unmoved(value));

        Ok(())
    }
    pub fn get_function(&self, name: &str) -> Option<&Function> {
        let (env, _) = self.resolve(name, 0..0).ok()?;

        match env.variables.get(name) {
            Some(Variable::Unmoved(_)) => None,
            Some(Variable::Moved) => None,
            Some(Variable::Function(function)) => Some(function),
            None => None,
        }
    }
    pub fn function_scope<T>(
        &mut self,
        name: &str,
        function: impl FnOnce(&mut Self, &Function) -> T,
    ) -> T {
        let (env, _) = self.resolve_mut(name, 0..0).unwrap();

        let return_result;
        let var = env.variables.get_mut(name);
        let fn_obj = match var {
            Some(Variable::Function(_)) => {
                let Variable::Function(fn_obj) = std::mem::replace(var.unwrap(), Variable::Moved)
                else {
                    unreachable!()
                };

                return_result = function(env, &fn_obj);

                fn_obj
            }
            _ => unreachable!(),
        };

        let var = env.variables.get_mut(name);
        let _ = std::mem::replace(var.unwrap(), Variable::Function(fn_obj));

        return_result
    }
    pub fn get(&self, name: &str, span: Span) -> Result<&Rc<RefCell<Value>>, RunError> {
        let (env, span) = self.resolve(name, span)?;

        match env.variables.get(name) {
            Some(Variable::Unmoved(value)) => Ok(value),
            Some(Variable::Moved) => Err(RunError::VariableMoved(span)),
            Some(Variable::Function(_)) => todo!(),
            None => Err(RunError::VariableNotFound(span)),
        }
    }
    pub fn move_var(&mut self, name: &str, span: Span) -> Result<Rc<RefCell<Value>>, RunError> {
        let (env, span) = self.resolve_mut(name, span)?;

        match env.variables.get_mut(name) {
            Some(Variable::Moved) => Err(RunError::VariableMoved(span)),
            Some(Variable::Function(_)) => todo!(),
            Some(reference) => {
                let Variable::Unmoved(value) = std::mem::replace(reference, Variable::Moved) else {
                    unreachable!()
                };

                Ok(value)
            }
            None => Err(RunError::VariableNotFound(span)),
        }
    }

    fn resolve(&self, name: &str, span: Span) -> Result<(&Self, Span), RunError> {
        if self.variables.contains_key(name) {
            return Ok((self, span));
        }

        match &self.parent {
            Some(parent) => parent.resolve(name, span),
            None => Err(RunError::VariableNotFound(span)),
        }
    }
    fn resolve_mut(&mut self, name: &str, span: Span) -> Result<(&mut Self, Span), RunError> {
        if self.variables.contains_key(name) {
            return Ok((self, span));
        }

        match &mut self.parent {
            Some(parent) => parent.resolve_mut(name, span),
            None => Err(RunError::VariableNotFound(span)),
        }
    }

    /// Registers a function for use inside the language.
    ///
    /// All parameters must implement [`TryFrom<Value>`].
    /// There is a hard limit of 8 parameters.
    ///
    /// The return value of the function must implement [`Into<Value>`]
    ///
    /// You should take a look at the [Standard Library](super::stdlib) for examples.
    pub fn register_fn<T>(
        &mut self,
        name: impl Into<String>,
        function: impl IntoFunction<T> + 'static,
    ) -> &mut Self {
        self.variables
            .insert(name.into(), Variable::Function(function.into_function()));

        self
    }
}
