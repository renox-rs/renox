//! Questions an app command asks when it runs in a terminal, like Laravel's
//! `$this->ask()`, `secret()`, `confirm()` and `choice()`.
//!
//! ```no_run
//! # use renox::prelude::*;
//! use renox::prompt;
//!
//! async fn create_admin(args: renox::command::Args, state: AppState) -> Result {
//!     let email = match args.value("--email") {
//!         Some(email) => email.to_owned(),
//!         None => prompt::ask("Email").await?,
//!     };
//!     let name = prompt::ask_or("Name", "Admin").await?;
//!     let password = prompt::secret("Password").await?;
//!     let role = prompt::choice("Role", &["admin", "staff"], Some("staff")).await?;
//!     if prompt::confirm(&format!("Create {email} as {role}?"), true).await? {
//!         User::register(&state.db, &name, &email, &password).await?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! Questions go to stderr. Without a terminal (a pipe, CI), answers are read
//! line by line from stdin; when stdin has none, a question with a default
//! takes it and one without fails, saying which answer was missing. Tests
//! answer with [`answering`].

use std::collections::VecDeque;
use std::future::Future;
use std::io::{BufRead, IsTerminal, Write};
use std::sync::Mutex;

use crate::{Error, Result};

tokio::task_local! {
    static ANSWERS: Mutex<VecDeque<String>>;
}

/// Runs `fut` (e.g. `app.kernel().call(…)`) with these answers, one per
/// question, in order, instead of asking anyone.
///
/// ```
/// # async fn demo(app: renox::testing::TestApp) -> renox::Result {
/// renox::prompt::answering(["ana@example.com", "secret-123", "yes"], app.kernel().call("admin:create", [""; 0])).await
/// # }
/// ```
pub async fn answering<F: Future>(
    answers: impl IntoIterator<Item = impl Into<String>>,
    fut: F,
) -> F::Output {
    let answers = answers.into_iter().map(Into::into).collect();
    ANSWERS.scope(Mutex::new(answers), fut).await
}

/// Where an answer comes from.
enum Line {
    Answer(String),
    /// Nothing to read: no answer given in a test, or stdin is closed.
    None,
}

async fn read(question: &str, hidden: bool) -> Result<(Line, bool)> {
    if let Ok(next) = ANSWERS.try_with(|answers| {
        answers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }) {
        return Ok((next.map_or(Line::None, Line::Answer), false));
    }
    let question = question.to_owned();
    let read = tokio::task::spawn_blocking(move || -> std::io::Result<(Line, bool)> {
        let interactive = std::io::stdin().is_terminal();
        if interactive && hidden {
            let answer = rpassword::prompt_password(format!("{question}: "))?;
            return Ok((Line::Answer(answer), true));
        }
        if interactive {
            eprint!("{question}: ");
            std::io::stderr().flush()?;
        }
        let mut line = String::new();
        let read = std::io::stdin().lock().read_line(&mut line)?;
        let answer = line.trim_end_matches(['\n', '\r']).to_owned();
        Ok((
            if read == 0 {
                Line::None
            } else {
                Line::Answer(answer)
            },
            interactive,
        ))
    });
    read.await
        .map_err(|err| Error::Internal(anyhow::anyhow!("the prompt failed: {err}")))?
        .map_err(|err| Error::Internal(anyhow::anyhow!("could not read the answer: {err}")))
}

fn unanswered(question: &str) -> Error {
    Error::Internal(anyhow::anyhow!(
        "no answer for \"{question}\": run the command in a terminal, or pass it as an option"
    ))
}

/// Asks until something is typed.
pub async fn ask(question: &str) -> Result<String> {
    loop {
        match read(question, false).await? {
            (Line::Answer(answer), _) if !answer.trim().is_empty() => {
                return Ok(answer.trim().to_owned());
            }
            (Line::Answer(_), true) => eprintln!("An answer is needed."),
            _ => return Err(unanswered(question)),
        }
    }
}

/// Asks, showing `default`, which an empty answer (or no terminal) takes.
pub async fn ask_or(question: &str, default: &str) -> Result<String> {
    match read(&format!("{question} [{default}]"), false).await? {
        (Line::Answer(answer), _) if !answer.trim().is_empty() => Ok(answer.trim().to_owned()),
        _ => Ok(default.to_owned()),
    }
}

/// Asks without showing what's typed, e.g. for a password.
pub async fn secret(question: &str) -> Result<String> {
    loop {
        match read(question, true).await? {
            (Line::Answer(answer), _) if !answer.is_empty() => return Ok(answer),
            (Line::Answer(_), true) => eprintln!("An answer is needed."),
            _ => return Err(unanswered(question)),
        }
    }
}

/// A yes/no question; an empty answer (or no terminal) takes `default`.
pub async fn confirm(question: &str, default: bool) -> Result<bool> {
    let hint = if default { "Y/n" } else { "y/N" };
    loop {
        let (line, interactive) = read(&format!("{question} [{hint}]"), false).await?;
        let Line::Answer(answer) = line else {
            return Ok(default);
        };
        match answer.trim().to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" | "ya" => return Ok(true),
            "n" | "no" => return Ok(false),
            other if !interactive => {
                return Err(Error::Internal(anyhow::anyhow!(
                    "\"{other}\" isn't yes or no (for \"{question}\")"
                )));
            }
            _ => eprintln!("Answer yes or no."),
        }
    }
}

/// One of `options`, by its text or its number (1, 2…); an empty answer takes
/// `default`.
pub async fn choice(question: &str, options: &[&str], default: Option<&str>) -> Result<String> {
    let mut listed = question.to_owned();
    for (i, option) in options.iter().enumerate() {
        listed.push_str(&format!("\n  {}. {option}", i + 1));
    }
    if let Some(default) = default {
        listed.push_str(&format!("\n[{default}]"));
    }
    loop {
        let (line, interactive) = read(&listed, false).await?;
        let answer = match line {
            Line::Answer(answer) if !answer.trim().is_empty() => answer.trim().to_owned(),
            _ => match default {
                Some(default) => return Ok(default.to_owned()),
                None if interactive => {
                    eprintln!("Pick one of the options.");
                    continue;
                }
                None => return Err(unanswered(question)),
            },
        };
        let picked = answer
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|i| options.get(i))
            .or_else(|| options.iter().find(|o| o.eq_ignore_ascii_case(&answer)));
        match picked {
            Some(option) => return Ok((*option).to_owned()),
            None if interactive => eprintln!("Pick one of the options."),
            None => {
                return Err(Error::Internal(anyhow::anyhow!(
                    "\"{answer}\" isn't one of {options:?} (for \"{question}\")"
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn answers_come_from_the_scope_in_order() {
        let (name, port, password, sure, role, number) =
            answering([" Ana ", "", "s3cret", "YES", "Staff", "1"], async {
                (
                    ask("Name").await.unwrap(),
                    ask_or("Port", "3000").await.unwrap(),
                    secret("Password").await.unwrap(),
                    confirm("Sure?", false).await.unwrap(),
                    choice("Role", &["admin", "staff"], None).await.unwrap(),
                    choice("Role", &["admin", "staff"], None).await.unwrap(),
                )
            })
            .await;
        assert_eq!(
            (name.as_str(), port.as_str(), password.as_str(), sure),
            ("Ana", "3000", "s3cret", true)
        );
        assert_eq!((role.as_str(), number.as_str()), ("staff", "admin"));
    }

    #[tokio::test]
    async fn missing_or_wrong_answers() {
        answering([""; 0], async {
            let err = ask("Email").await.unwrap_err();
            assert!(format!("{err:?}").contains("no answer for \"Email\""));
            assert!(!confirm("Sure?", false).await.unwrap(), "the default");
            assert_eq!(choice("Size", &["s", "m"], Some("m")).await.unwrap(), "m");
        })
        .await;
        answering(["maybe", "xl", ""], async {
            assert!(confirm("Sure?", true).await.is_err());
            assert!(choice("Size", &["s", "m"], None).await.is_err());
            assert!(ask("Name").await.is_err(), "empty and not interactive");
        })
        .await;
    }
}
