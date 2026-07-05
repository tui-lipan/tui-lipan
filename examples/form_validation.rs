//! Form validation example demonstrating Validator and Input.error().
//!
//! Run with: cargo run --example form_validation

use std::sync::Arc;

use tui_lipan::prelude::*;

const LOGIN_MIN_WIDTH: u16 = 60;

struct LoginForm {
    username: Arc<str>,
    username_cursor: usize,
    username_anchor: Option<usize>,
    password: Arc<str>,
    password_cursor: usize,
    password_anchor: Option<usize>,
    username_error: Option<Arc<str>>,
    password_error: Option<Arc<str>>,
    submitted: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    UsernameChanged(InputEvent),
    PasswordChanged(InputEvent),
    Submit,
    Reset,
}

impl LoginForm {
    fn new() -> Self {
        Self {
            username: Arc::from(""),
            username_cursor: 0,
            username_anchor: None,
            password: Arc::from(""),
            password_cursor: 0,
            password_anchor: None,
            username_error: None,
            password_error: None,
            submitted: false,
        }
    }

    fn username_validator() -> StringValidator {
        StringValidator::new()
            .required(Arc::from("Username is required"))
            .min_length(4, Arc::from("Username must be at least 4 characters"))
            .max_length(24, Arc::from("Username must be 24 characters or fewer"))
            .rule(|value| {
                if value
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
                {
                    Ok(())
                } else {
                    Err(ValidationError {
                        message: Arc::from(
                            "Username may only use letters, numbers, '.', '_' or '-'",
                        ),
                    })
                }
            })
    }

    fn password_validator() -> StringValidator {
        StringValidator::new()
            .required(Arc::from("Password is required"))
            .min_length(8, Arc::from("Password must be at least 8 characters"))
            .max_length(64, Arc::from("Password must be 64 characters or fewer"))
            .rule(|value| {
                if value.chars().any(|ch| ch.is_ascii_uppercase()) {
                    Ok(())
                } else {
                    Err(ValidationError {
                        message: Arc::from("Password must include an uppercase letter"),
                    })
                }
            })
            .rule(|value| {
                if value.chars().any(|ch| ch.is_ascii_lowercase()) {
                    Ok(())
                } else {
                    Err(ValidationError {
                        message: Arc::from("Password must include a lowercase letter"),
                    })
                }
            })
            .rule(|value| {
                if value.chars().any(|ch| ch.is_ascii_digit()) {
                    Ok(())
                } else {
                    Err(ValidationError {
                        message: Arc::from("Password must include a number"),
                    })
                }
            })
            .rule(|value| {
                if value.chars().any(|ch| !ch.is_ascii_alphanumeric()) {
                    Ok(())
                } else {
                    Err(ValidationError {
                        message: Arc::from("Password must include a symbol"),
                    })
                }
            })
    }

    fn validate(&mut self) -> bool {
        self.username_error = Self::username_validator()
            .validate(&self.username)
            .err()
            .map(|e| e.message);
        self.password_error = Self::password_validator()
            .validate(&self.password)
            .err()
            .map(|e| e.message);
        self.username_error.is_none() && self.password_error.is_none()
    }
}

fn login_rules_text() -> &'static str {
    "Login rules:\n• 4-24 characters\n• letters, numbers, '.', '_' or '-'\n\nPassword rules:\n• 8-64 characters\n• at least one uppercase letter\n• at least one lowercase letter\n• at least one number\n• at least one symbol"
}

impl Component for LoginForm {
    type Message = Msg;
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::UsernameChanged(ev) => {
                self.username = ev.value;
                self.username_cursor = ev.cursor;
                self.username_anchor = ev.anchor;
                // Clear error on change, validate on submit
                if self.username_error.is_some() {
                    self.username_error = Self::username_validator()
                        .validate(&self.username)
                        .err()
                        .map(|e| e.message);
                }
                Update::full()
            }
            Msg::PasswordChanged(ev) => {
                self.password = ev.value;
                self.password_cursor = ev.cursor;
                self.password_anchor = ev.anchor;
                if self.password_error.is_some() {
                    self.password_error = Self::password_validator()
                        .validate(&self.password)
                        .err()
                        .map(|e| e.message);
                }
                Update::full()
            }
            Msg::Submit => {
                if self.validate() {
                    self.submitted = true;
                }
                Update::full()
            }
            Msg::Reset => {
                self.username = Arc::from("");
                self.username_cursor = 0;
                self.username_anchor = None;
                self.password = Arc::from("");
                self.password_cursor = 0;
                self.password_anchor = None;
                self.username_error = None;
                self.password_error = None;
                self.submitted = false;
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let all_valid = Self::username_validator().validate(&self.username).is_ok()
            && Self::password_validator().validate(&self.password).is_ok();

        if self.submitted {
            return Center::new()
                .child(
                    VStack::new()
                        .width(Length::Auto)
                        .gap(1)
                        .align(Align::Center)
                        .child(
                            Text::new("Login successful!")
                                .style(Style::new().fg(Color::Green).bold()),
                        )
                        .child(Text::new(format!("Welcome, {}!", self.username)))
                        .child(Button::new("Reset").on_click(ctx.link().callback(|_| Msg::Reset))),
                )
                .into();
        }

        let username_input = Element::from(
            Input::new(&*self.username)
                .cursor(self.username_cursor)
                .anchor(self.username_anchor)
                .placeholder("Enter username")
                .on_change(ctx.link().callback(Msg::UsernameChanged))
                .on_key(ctx.link().key_handler(|key| match key.code {
                    KeyCode::Enter => Some(Msg::Submit),
                    _ => None,
                }))
                .error(self.username_error.clone())
                .reserve_error_row(true)
                .width(Length::Auto),
        )
        .min_width(Length::Px(LOGIN_MIN_WIDTH));

        let password_input = Element::from(
            Input::new(&*self.password)
                .cursor(self.password_cursor)
                .anchor(self.password_anchor)
                .placeholder("Enter password")
                .mask(Some('●'))
                .on_change(ctx.link().callback(Msg::PasswordChanged))
                .on_key(ctx.link().key_handler(|key| match key.code {
                    KeyCode::Enter => Some(Msg::Submit),
                    _ => None,
                }))
                .error(self.password_error.clone())
                .reserve_error_row(true)
                .width(Length::Auto),
        )
        .min_width(Length::Px(LOGIN_MIN_WIDTH));

        Center::new()
            .child(
                VStack::new()
                    .width(Length::Auto)
                    .height(Length::Auto)
                    .gap(1)
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .child(Text::new("Welcome back").style(Style::new().bold()))
                    .child(
                        Text::new("Sign in with a stronger demo validation policy.")
                            .style(Style::new().fg(Color::DarkGray)),
                    )
                    .child(
                        VStack::new()
                            .width(Length::Auto)
                            .height(Length::Auto)
                            .gap(1)
                            .align(Align::Center)
                            .child(
                                VStack::new()
                                    .width(Length::Auto)
                                    .height(Length::Auto)
                                    .gap(0)
                                    .child(Text::new("Username").style(Style::new().bold()))
                                    .child(username_input),
                            )
                            .child(
                                VStack::new()
                                    .width(Length::Auto)
                                    .height(Length::Auto)
                                    .gap(0)
                                    .child(Text::new("Password").style(Style::new().bold()))
                                    .child(password_input),
                            )
                            .child(
                                Button::new("Login")
                                    .disabled(!all_valid)
                                    .style(Style::new().bold())
                                    .disabled_style(Style::new().dim())
                                    .on_click(ctx.link().callback(|_| Msg::Submit)),
                            ),
                    )
                    .child(
                        Text::new(login_rules_text())
                            .style(Style::new().fg(Color::DarkGray))
                            .width(Length::Auto),
                    ),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Form Validation Demo")
        .mount(LoginForm::new())
        .run()
}
