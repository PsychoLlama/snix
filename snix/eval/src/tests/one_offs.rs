use crate::*;
use pretty_assertions::assert_matches;
use rustc_hash::FxHashMap;
use std::{
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
};

#[test]
fn test_source_builtin() {
    // Test an evaluation with a source-only builtin. The test ensures
    // that the artificially constructed thunking is correct.

    let eval = Evaluation::builder_pure()
        .add_src_builtin("testSourceBuiltin", "42")
        .build();

    let result = eval.evaluate("builtins.testSourceBuiltin", None);
    assert!(
        result.errors.is_empty(),
        "evaluation failed: {:?}",
        result.errors
    );

    let value = result.value.unwrap();
    assert_matches!(value, Value::Integer(i) if i == 42);
}

#[test]
fn skip_broken_bytecode() {
    let result = Evaluation::builder_pure()
        .build()
        .evaluate(/* code = */ "x", None);

    assert_eq!(result.errors.len(), 1);
    assert_matches!(result.errors[0].kind, ErrorKind::UnknownStaticVariable);
}

/// Checks that deep forcing happens in lexicographic key order
/// See https://cl.snix.dev/c/snix/+/30309/comment/a7c9c6d5_bacf7332/ for
/// details.
#[test]
fn key_order_deep_force() {
    let result = Evaluation::builder_pure().build().evaluate(
        /* code = */
        r#"builtins.toXML {
          c = throw "ccc";
          a = throw "aaa";
          b = throw "bbb";
          d = throw "dd";
        }"#,
        None,
    );
    assert_eq!(result.errors.len(), 1);

    assert_matches!(
        &result.errors[0].kind,
        ErrorKind::CatchableError(CatchableErrorKind::Throw(s)) if s == &NixString::from("aaa")
    );
}

/// An in-memory filesystem. None of its paths exist on disk, which catches
/// code that bypasses [`EvalIO`] and probes `std::fs` directly.
struct MemoryIO(FxHashMap<PathBuf, &'static str>);

impl MemoryIO {
    fn new(files: &[(&str, &'static str)]) -> Self {
        Self(
            files
                .iter()
                .map(|(path, contents)| (PathBuf::from(path), *contents))
                .collect(),
        )
    }
}

impl EvalIO for MemoryIO {
    fn path_exists(&self, path: &Path) -> io::Result<bool> {
        Ok(self.0.keys().any(|file| file.starts_with(path)))
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn io::Read>> {
        let contents = self.0.get(path).ok_or(io::ErrorKind::NotFound)?;
        Ok(Box::new(contents.as_bytes()))
    }

    fn file_type(&self, path: &Path) -> io::Result<FileType> {
        if self.0.contains_key(path) {
            Ok(FileType::Regular)
        } else if self.path_exists(path)? {
            Ok(FileType::Directory)
        } else {
            Err(io::ErrorKind::NotFound.into())
        }
    }

    fn read_dir(&self, _: &Path) -> io::Result<Vec<(bytes::Bytes, FileType)>> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn import_path(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_owned())
    }

    fn get_env(&self, _: &OsStr) -> Option<OsString> {
        None
    }
}

fn evaluate_in_memory(code: &str, files: &[(&str, &'static str)]) -> EvaluationResult {
    Evaluation::builder(Box::new(MemoryIO::new(files)) as Box<dyn EvalIO>)
        .enable_import()
        .build()
        .evaluate(code, Some("/".into()))
}

#[test]
fn import_directory_through_io_handle() {
    let result = evaluate_in_memory(
        "import /snix-test/lib",
        &[("/snix-test/lib/default.nix", "42")],
    );

    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_matches!(result.value, Some(Value::Integer(42)));
}
