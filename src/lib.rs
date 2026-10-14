use std::{
    collections::HashMap,
    io::{self, Write},
    process::{Command, Stdio},
};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug, thiserror::Error)]
pub enum InvokeError {
    #[error("function not found for name {0}")]
    NotFound(String),
    #[error("name already exists: {0}")]
    Duplicate(String),
    #[error("Command errored out: {code} ({error})")]
    Failed { code: i32, error: String },
    #[error("io error: {0}")]
    IoError(#[from] io::Error),
    #[error("The function call timed out")]
    Timeout,
    #[error("function call panicked")]
    InternalError,
}

impl IntoResponse for InvokeError {
    fn into_response(self) -> Response {
        let status = match &self {
            InvokeError::NotFound(_) => StatusCode::NOT_FOUND,
            InvokeError::Duplicate(_) => StatusCode::CONFLICT,
            InvokeError::Failed { code: _, error: _ } => StatusCode::INTERNAL_SERVER_ERROR,
            InvokeError::IoError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            InvokeError::Timeout => StatusCode::GATEWAY_TIMEOUT,
            InvokeError::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, format!("{:?}", self.to_string())).into_response()
    }
}

pub trait Data {
    fn invoke(&self, bytes: &[u8]) -> Result<Vec<u8>, InvokeError>;
}

pub struct Unit;

impl Data for Unit {
    fn invoke(&self, bytes: &[u8]) -> Result<Vec<u8>, InvokeError> {
        Ok(bytes.to_ascii_uppercase())
    }
}

pub struct Process {
    pub command: String,
    pub args: Vec<String>,
}

impl Data for Process {
    fn invoke(&self, bytes: &[u8]) -> Result<Vec<u8>, InvokeError> {
        let mut child = Command::new(&self.command)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        {
            let mut stdin = child.stdin.take().expect("stdin was piped");
            stdin.write_all(bytes)?;
        }

        let output = child.wait_with_output()?;

        if !output.status.success() {
            return Err(InvokeError::Failed {
                code: output.status.code().unwrap_or(-1),
                error: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(output.stdout)
    }
}

pub type DynamicData = Box<dyn Data + Send + Sync>;

pub struct Registry {
    pub registry: HashMap<String, DynamicData>,
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            registry: HashMap::new(),
        }
    }
    pub fn register<F: Data + Send + Sync + 'static>(
        &mut self,
        name: String,
        f: F,
    ) -> Result<(), InvokeError> {
        if self.registry.contains_key(&name) {
            return Err(InvokeError::Duplicate(name));
        }
        self.registry.insert(name, Box::new(f));
        Ok(())
    }
    pub fn list(&self) -> Vec<String> {
        let mut res: Vec<String> = self.registry.keys().cloned().collect();
        res.sort();
        res
    }
    pub fn invoke_by_name(&self, name: &str, bytes: &[u8]) -> Result<Vec<u8>, InvokeError> {
        let function = self.registry.get(name);
        match function {
            Some(res) => res.invoke(bytes),
            None => Err(InvokeError::NotFound(String::from(name))),
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works() {
        let input: &[u8] = b"hello, world";
        let res = Unit.invoke(input).unwrap();

        assert_eq!(res, b"HELLO, WORLD")
    }

    #[test]
    fn input_works() {
        let i = Process {
            command: String::from("tr"),
            args: vec![String::from("a-z"), String::from("A-Z")],
        };

        let res = i.invoke(b"hello").unwrap();

        assert_eq!(res, b"HELLO")
    }

    #[test]
    fn input_errors_out() {
        let i = Process {
            command: String::from("sh"),
            args: vec![String::from("-c"), String::from("exit 3")],
        };

        let res = i.invoke(b"hello");

        assert!(res.is_err());

        let err = res.unwrap_err();

        assert!(matches!(err, InvokeError::Failed { code: 3, .. }))
    }

    #[test]
    fn register() {
        let unit = Unit;
        let process = Process {
            command: String::from("sh"),
            args: vec![String::from("-c"), String::from("exit 3")],
        };
        let mut r = Registry::new();
        r.register(String::from("unit"), unit).unwrap();
        r.register(String::from("process"), process).unwrap();

        assert!(r.registry.contains_key("unit"));
        assert!(r.registry.contains_key("process"));
    }

    #[test]
    fn register_duplicate() {
        let unit = Unit;
        let mut registry = Registry::new();

        registry.register(String::from("unit"), unit).unwrap();
        let unit2 = Unit;
        let res = registry.register(String::from("unit"), unit2);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(matches!(err, InvokeError::Duplicate(ref n) if n == "unit"))
    }

    #[test]
    fn invoke_existing_fn() {
        let unit = Unit;
        let mut registry = Registry::new();
        registry.register(String::from("unit"), unit).unwrap();

        let res = registry.invoke_by_name("unit", b"hello").unwrap();
        assert_eq!(res, b"HELLO")
    }
    #[test]
    fn list_is_sorted() {
        let mut registry = Registry::new();
        registry.register(String::from("zeta"), Unit).unwrap();
        registry
            .register(
                String::from("alpha"),
                Process {
                    command: String::from("cat"),
                    args: vec![],
                },
            )
            .unwrap();
        registry.register(String::from("mid"), Unit).unwrap();

        assert_eq!(registry.list(), vec!["alpha", "mid", "zeta"]);
    }

    #[test]
    fn invoke_process_by_name() {
        let mut registry = Registry::new();
        registry
            .register(
                String::from("upper"),
                Process {
                    command: String::from("tr"),
                    args: vec![String::from("a-z"), String::from("A-Z")],
                },
            )
            .unwrap();

        let res = registry.invoke_by_name("upper", b"hello").unwrap();
        assert_eq!(res, b"HELLO");
    }

    #[test]
    fn invoke_missing_fn() {
        let registry = Registry::new();

        let res = registry.invoke_by_name("nope", b"hello");
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(matches!(err, InvokeError::NotFound(ref n) if n == "nope"));
    }
}
