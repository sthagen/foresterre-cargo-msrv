use crate::io::SendWriter;
use crate::reporter::Message;
use crate::Action;
use std::cell::Cell;
use std::io::{Stderr, Stdout};
use std::sync::{Arc, Mutex, MutexGuard};
use std::{fmt, io};
use storyteller::EventHandler;

/// An output handler which reports just some minimal results.
///
/// It can be used when machine parsing is used, but parsing json would be too much work
/// or there is no desire for the extended output which can be found in the json output.
///
/// Ensure the success and failure writer are not locked at the same time.
#[derive(Debug)]
pub struct MinimalOutputHandler<S: SendWriter, F: SendWriter> {
    success_writer: Arc<Mutex<S>>,
    failure_writer: Arc<Mutex<F>>, // should we split the writer ??
}

impl<S: SendWriter, F: SendWriter> MinimalOutputHandler<S, F> {
    fn new(success_writer: S, failure_writer: F) -> Self {
        Self {
            success_writer: Arc::new(Mutex::new(success_writer)),
            failure_writer: Arc::new(Mutex::new(failure_writer)),
        }
    }
}

impl MinimalOutputHandler<Stdout, Stderr> {
    pub fn stderr() -> Self {
        Self {
            success_writer: Arc::new(Mutex::new(io::stdout())),
            failure_writer: Arc::new(Mutex::new(io::stderr())),
        }
    }
}

#[cfg(test)]
impl<S: SendWriter, F: SendWriter> MinimalOutputHandler<S, F> {
    fn inner_success_writer(&self) -> MutexGuard<'_, S> {
        self.success_writer
            .lock()
            .expect("Unable to lock success_writer")
    }

    fn inner_failure_writer(&self) -> MutexGuard<'_, F> {
        self.failure_writer
            .lock()
            .expect("Unable to lock failure_writer")
    }
}

impl<S: SendWriter, F: SendWriter> EventHandler for MinimalOutputHandler<S, F> {
    type Event = super::Event;

    fn handle(&self, event: Self::Event) {
        macro_rules! success_writeln {
            ($($arg:tt)*) => {{
                let mut writer = self.success_writer.lock().expect("Unable to lock success_writer");
                writeln!(&mut writer, $($arg)*);
            }};
        }

        macro_rules! failure_writeln {
            ($($arg:tt)*) => {{
                let mut writer = self.failure_writer.lock().expect("Unable to lock failure_writer");
                writeln!(&mut writer, $($arg)*);
            }};
        }

        use std::io::Write;

        // Early return when message is not a final result message.
        // Also ensures we don't unnecessarily lock the writer.
        if !event.message().is_final_result() {
            return;
        }

        match event.message() {
            // cargo msrv (find)
            Message::MsrvResult(find) => match find.msrv() {
                Some(v) => {
                    success_writeln!("{}", v)
                }
                None => failure_writeln!("{}", "none"),
            },
            // cargo msrv list
            // Consider: simplify output for this command, if you want the full output you can use
            //           the default human output mode
            //           for now: unsupported
            Message::ListDep(list) => {
                failure_writeln!("unsupported")
            }
            // cargo msrv set output
            Message::SetOutput(set) => {
                success_writeln!("{}", set.version())
            }
            // cargo msrv show
            Message::ShowOutput(show) => {
                success_writeln!("{}", show.version())
            } // cargo msrv verify
            Message::Verify(verify) if verify.is_compatible() => {
                success_writeln!("{}", verify.is_compatible())
            }
            Message::Verify(verify) if !verify.is_compatible() => {
                failure_writeln!("{}", verify.is_compatible())
            }
            // If not a final result, discard
            _ => {
                unreachable!("Early return missing, see `Message::is_final_result`.")
            }
        };
    }
}

impl Message {
    fn is_final_result(&self) -> bool {
        matches!(
            &self,
            Message::MsrvResult(_)
                | Message::ListDep(_)
                | Message::SetOutput(_)
                | Message::SetOutput(_)
                | Message::ShowOutput(_)
                | Message::Verify(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::config::list::ListMsrvVariant;
    use crate::dependency_graph::DependencyGraph;
    use crate::manifest::bare_version::BareVersion;
    use crate::reporter::event::{
        ListDep, MsrvResult, Progress, SetOutputMessage, ShowOutputMessage, VerifyOutput,
    };
    use crate::reporter::handler::minimal_output_handler::MinimalOutputHandler;
    use crate::reporter::{Message, ReporterSetup, TestReporter};
    use crate::toolchain::OwnedToolchainSpec;
    use crate::{semver, Action, Config, Event};
    use cargo_metadata::PackageId;
    use serde::Deserialize;
    use std::convert::TryInto;
    use std::path::Path;
    use std::sync::Arc;
    use storyteller::{EventHandler, EventListener, FinishProcessing, Reporter};

    #[test]
    fn find_with_result() {
        let config = Config::new(Action::Find, "my-target");
        let min_available = BareVersion::ThreeComponents(1, 0, 0);
        let max_available = BareVersion::ThreeComponents(2, 0, 0);

        let event = MsrvResult::new_msrv(
            semver::Version::new(1, 10, 100),
            &config,
            min_available,
            max_available,
        );

        let s = Vec::new();
        let f = Vec::new();
        let handler = MinimalOutputHandler::new(s, f);
        handler.handle(event.into());

        let s = handler.inner_success_writer().clone();
        let s = String::from_utf8_lossy(&s);
        assert_eq!(s.as_ref(), "1.10.100\n");

        let f = handler.inner_failure_writer().clone();
        let f = String::from_utf8_lossy(&f);
        assert_eq!(f.as_ref(), "");
    }

    #[test]
    fn find_without_result() {
        let config = Config::new(Action::Find, "my-target");
        let min_available = BareVersion::ThreeComponents(1, 0, 0);
        let max_available = BareVersion::ThreeComponents(2, 0, 0);

        let event = MsrvResult::none(&config, min_available, max_available);

        let s = Vec::new();
        let f = Vec::new();
        let handler = MinimalOutputHandler::new(s, f);
        handler.handle(event.into());

        let s = handler.inner_success_writer().clone();
        let s = String::from_utf8_lossy(&s);
        assert_eq!(s.as_ref(), "");

        let f = handler.inner_failure_writer().clone();
        let f = String::from_utf8_lossy(&f);
        assert_eq!(f.as_ref(), "none\n");
    }

    #[test]
    fn list_direct_deps() {
        let config = Config::new(Action::Find, "my-target");
        let min_available = BareVersion::ThreeComponents(1, 0, 0);
        let max_available = BareVersion::ThreeComponents(2, 0, 0);

        let package_id = PackageId {
            repr: "hello_world".to_string(),
        };
        let dep_graph = DependencyGraph::empty(package_id);
        let event = ListDep::new(ListMsrvVariant::DirectDeps, dep_graph);

        let s = Vec::new();
        let f = Vec::new();
        let handler = MinimalOutputHandler::new(s, f);
        handler.handle(event.into());

        let s = handler.inner_success_writer().clone();
        let s = String::from_utf8_lossy(&s);
        assert_eq!(s.as_ref(), "");

        let f = handler.inner_failure_writer().clone();
        let f = String::from_utf8_lossy(&f);
        assert_eq!(f.as_ref(), "unsupported\n");
    }

    #[test]
    fn set_output() {
        let event = SetOutputMessage::new(
            BareVersion::TwoComponents(1, 20),
            Path::new("/my/path").to_path_buf(),
        );

        let s = Vec::new();
        let f = Vec::new();
        let handler = MinimalOutputHandler::new(s, f);
        handler.handle(event.into());

        let s = handler.inner_success_writer().clone();
        let s = String::from_utf8_lossy(&s);
        assert_eq!(s.as_ref(), "1.20\n");

        let f = handler.inner_failure_writer().clone();
        let f = String::from_utf8_lossy(&f);
        assert_eq!(f.as_ref(), "");
    }

    #[test]
    fn show_output() {
        let event = ShowOutputMessage::new(
            BareVersion::ThreeComponents(1, 40, 3),
            Path::new("/my/path").to_path_buf(),
        );

        let s = Vec::new();
        let f = Vec::new();
        let handler = MinimalOutputHandler::new(s, f);
        handler.handle(event.into());

        let output = handler.inner_failure_writer().clone();
        let s = handler.inner_success_writer().clone();
        let s = String::from_utf8_lossy(&s);
        assert_eq!(s.as_ref(), "1.40.3\n");

        let f = handler.inner_failure_writer().clone();
        let f = String::from_utf8_lossy(&f);
        assert_eq!(f.as_ref(), "");
    }

    #[test]
    fn verify_true() {
        let event = VerifyOutput::compatible(OwnedToolchainSpec::new(
            &semver::Version::new(1, 2, 3),
            "test_target",
        ));

        let s = Vec::new();
        let f = Vec::new();
        let handler = MinimalOutputHandler::new(s, f);
        handler.handle(event.into());

        let output = handler.inner_failure_writer().clone();
        let s = handler.inner_success_writer().clone();
        let s = String::from_utf8_lossy(&s);
        assert_eq!(s.as_ref(), "true\n");

        let f = handler.inner_failure_writer().clone();
        let f = String::from_utf8_lossy(&f);
        assert_eq!(f.as_ref(), "");
    }

    #[test]
    fn verify_false_no_error_message() {
        let event = VerifyOutput::incompatible(
            OwnedToolchainSpec::new(&semver::Version::new(1, 2, 3), "test_target"),
            None,
        );

        let s = Vec::new();
        let f = Vec::new();
        let handler = MinimalOutputHandler::new(s, f);
        handler.handle(event.into());

        let output = handler.inner_failure_writer().clone();
        let s = handler.inner_success_writer().clone();
        let s = String::from_utf8_lossy(&s);
        assert_eq!(s.as_ref(), "");

        let f = handler.inner_failure_writer().clone();
        let f = String::from_utf8_lossy(&f);
        assert_eq!(f.as_ref(), "false\n");
    }

    #[test]
    fn verify_false_with_error_message() {
        let event = VerifyOutput::incompatible(
            OwnedToolchainSpec::new(&semver::Version::new(1, 2, 3), "test_target"),
            Some("error message".to_string()),
        );

        let s = Vec::new();
        let f = Vec::new();
        let handler = MinimalOutputHandler::new(s, f);
        handler.handle(event.into());

        let output = handler.inner_failure_writer().clone();
        let s = handler.inner_success_writer().clone();
        let s = String::from_utf8_lossy(&s);
        assert_eq!(s.as_ref(), "");

        let f = handler.inner_failure_writer().clone();
        let f = String::from_utf8_lossy(&f);
        assert_eq!(f.as_ref(), "false\n");
    }

    #[test]
    fn unreported_event() {
        let event = Progress::new(1, 100, 50);

        let s = Vec::new();
        let f = Vec::new();
        let handler = MinimalOutputHandler::new(s, f);
        handler.handle(event.into());

        let output = handler.inner_failure_writer().clone();
        let s = handler.inner_success_writer().clone();
        let s = String::from_utf8_lossy(&s);
        assert_eq!(s.as_ref(), "");

        let f = handler.inner_failure_writer().clone();
        let f = String::from_utf8_lossy(&f);
        assert_eq!(f.as_ref(), "");
    }
}
