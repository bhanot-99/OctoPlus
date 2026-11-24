//! Notifications rendered only from fixed templates (FR-NUD-12, R0-6, design.md §8.2).

use std::collections::HashMap;
use std::sync::Arc;

use octoplus_proto::v1::TemplateArg;
use tokio::sync::Mutex;
use zbus::Variant;

/// Rendered notification text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub title: String,
    pub body: String,
}

/// Errors from notification rendering or delivery.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NotifyError {
    #[error("unknown template id")]
    UnknownTemplate,
    #[error("mismatched argument names")]
    BadArguments,
    #[error("argument value not finite or out of range")]
    BadValue,
    #[error("notification delivery failed")]
    DeliveryFailed,
}

/// A fixed notification template (design.md §8.2).
#[derive(Debug, Clone, Copy)]
pub struct Template {
    pub id: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    pub args: &'static [&'static str],
}

/// Compiled-in notification templates (design.md §8.2, FR-NUD-12).
pub const TEMPLATES: &[Template] = &[
    Template {
        id: "disk.nearly_full",
        title: "Disk nearly full",
        body: "Your disk is {percent} percent full.",
        args: &["percent"],
    },
    Template {
        id: "disk.critical",
        title: "Disk almost full",
        body: "Your disk is almost full — {free_gib} GiB left.",
        args: &["free_gib"],
    },
    Template {
        id: "disk.filling_fast",
        title: "Disk filling fast",
        body: "At this rate your disk is full in about {hours} hours.",
        args: &["hours"],
    },
    Template {
        id: "battery.low",
        title: "Battery low",
        body: "Battery's at {percent} percent and you're not plugged in.",
        args: &["percent"],
    },
    Template {
        id: "thermal.hot",
        title: "Running hot",
        body: "Your laptop's been running hot for a while.",
        args: &[],
    },
    Template {
        id: "memory.pressure",
        title: "Memory nearly full",
        body: "Memory is nearly full, so things may feel slow.",
        args: &[],
    },
    Template {
        id: "session.long",
        title: "Time for a break?",
        body: "You've been at it for {minutes} minutes. Might be time for a short break.",
        args: &["minutes"],
    },
    Template {
        id: "part.degraded",
        title: "OctoPlus degraded",
        body: "Part of OctoPlus isn't working right now, so it may miss things.",
        args: &[],
    },
    Template {
        id: "nudge.while_away",
        title: "While you were away",
        body: "{count} things needed your attention while you were away or busy.",
        args: &["count"],
    },
    Template {
        id: "nudge.test",
        title: "Test nudge",
        body: "This is a test of the nudge system. Nothing needs attention.",
        args: &[],
    },
    Template {
        id: "cleanup.done",
        title: "Cleanup finished",
        body: "Freed {freed_gib} GiB.",
        args: &["freed_gib"],
    },
];

/// Formats a finite number: whole integers as e.g. `92`, otherwise one decimal e.g. `4.2`.
fn format_number(val: f64) -> String {
    if val.fract() == 0.0 {
        #[allow(clippy::cast_possible_truncation)]
        let int_val = val as i64;
        format!("{int_val}")
    } else {
        format!("{val:.1}")
    }
}

/// Renders a notification from a registered template.
///
/// # Errors
/// - [`NotifyError::UnknownTemplate`] if `template_id` is unknown.
/// - [`NotifyError::BadArguments`] if argument names do not match the template exactly (missing, extra, or duplicate).
/// - [`NotifyError::BadValue`] if any argument is non-finite (NaN, inf) or outside ±1e12.
pub fn render(template_id: &str, args: &[TemplateArg]) -> Result<Rendered, NotifyError> {
    let tmpl = TEMPLATES
        .iter()
        .find(|t| t.id == template_id)
        .ok_or(NotifyError::UnknownTemplate)?;

    if args.len() != tmpl.args.len() {
        return Err(NotifyError::BadArguments);
    }

    let mut values: HashMap<&str, String> = HashMap::with_capacity(args.len());

    for arg in args {
        if !tmpl.args.contains(&arg.name.as_str()) {
            return Err(NotifyError::BadArguments);
        }
        if !arg.value.is_finite() || arg.value.abs() > 1e12 {
            return Err(NotifyError::BadValue);
        }
        if values.insert(&arg.name, format_number(arg.value)).is_some() {
            // Duplicate argument name
            return Err(NotifyError::BadArguments);
        }
    }

    let mut body = tmpl.body.to_string();
    for &expected_arg in tmpl.args {
        let val_str = values.get(expected_arg).ok_or(NotifyError::BadArguments)?;
        let needle = format!("{{{expected_arg}}}");
        body = body.replace(&needle, val_str);
    }

    Ok(Rendered {
        title: tmpl.title.to_string(),
        body,
    })
}

#[zbus::proxy(
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications",
    interface = "org.freedesktop.Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: &HashMap<&str, Variant<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// Best-effort session bus notification sender.
#[derive(Debug, Clone)]
pub struct Notifier {
    fixed_conn: Option<zbus::Connection>,
    session_conn: Arc<Mutex<Option<zbus::Connection>>>,
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Notifier {
    /// Creates a new notifier connecting lazily to the session bus.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fixed_conn: None,
            session_conn: Arc::new(Mutex::new(None)),
        }
    }

    /// Creates a notifier with a fixed connection (useful for tests or custom bus).
    #[must_use]
    pub fn with_connection(conn: zbus::Connection) -> Self {
        Self {
            fixed_conn: Some(conn),
            session_conn: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_connection(&self) -> Result<zbus::Connection, NotifyError> {
        if let Some(ref conn) = self.fixed_conn {
            return Ok(conn.clone());
        }

        let mut guard = self.session_conn.lock().await;
        if let Some(ref conn) = *guard {
            return Ok(conn.clone());
        }

        let conn = zbus::Connection::session()
            .await
            .map_err(|_| NotifyError::DeliveryFailed)?;
        *guard = Some(conn.clone());
        Ok(conn)
    }

    /// Sends a rendered notification over D-Bus to `org.freedesktop.Notifications`.
    ///
    /// # Errors
    /// Returns [`NotifyError::DeliveryFailed`] on bus or RPC error.
    pub async fn deliver(&self, rendered: &Rendered) -> Result<u32, NotifyError> {
        self.deliver_with(rendered, false).await
    }

    /// Like [`Notifier::deliver`], optionally without a sound (`silent`: the standard
    /// `suppress-sound` hint).
    ///
    /// # Errors
    /// Returns [`NotifyError::DeliveryFailed`] on bus or RPC error.
    pub async fn deliver_with(
        &self,
        rendered: &Rendered,
        silent: bool,
    ) -> Result<u32, NotifyError> {
        let conn = self.get_connection().await?;
        let proxy = NotificationsProxy::new(&conn)
            .await
            .map_err(|_| NotifyError::DeliveryFailed)?;

        let mut hints: HashMap<&str, Variant<'_>> = HashMap::new();
        hints.insert("urgency", Variant::from(1u8));
        hints.insert("desktop-entry", Variant::from("org.octoplus.Confirm"));
        if silent {
            hints.insert("suppress-sound", Variant::from(true));
        }

        let res = proxy
            .notify(
                "OctoPlus",
                0,
                "",
                &rendered.title,
                &rendered.body,
                &[],
                &hints,
                -1,
            )
            .await;

        match res {
            Ok(id) => Ok(id),
            Err(e) => {
                tracing::warn!("notification delivery failed: {e}");
                if self.fixed_conn.is_none() {
                    let mut guard = self.session_conn.lock().await;
                    *guard = None;
                }
                Err(NotifyError::DeliveryFailed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_all_templates() {
        // 1. disk.nearly_full
        let r = render(
            "disk.nearly_full",
            &[TemplateArg {
                name: "percent".into(),
                value: 92.0,
            }],
        )
        .unwrap();
        assert_eq!(r.title, "Disk nearly full");
        assert_eq!(r.body, "Your disk is 92 percent full.");

        // 2. disk.critical
        let r = render(
            "disk.critical",
            &[TemplateArg {
                name: "free_gib".into(),
                value: 1.5,
            }],
        )
        .unwrap();
        assert_eq!(r.title, "Disk almost full");
        assert_eq!(r.body, "Your disk is almost full — 1.5 GiB left.");

        // 3. disk.filling_fast
        let r = render(
            "disk.filling_fast",
            &[TemplateArg {
                name: "hours".into(),
                value: 3.0,
            }],
        )
        .unwrap();
        assert_eq!(r.title, "Disk filling fast");
        assert_eq!(r.body, "At this rate your disk is full in about 3 hours.");

        // 4. battery.low
        let r = render(
            "battery.low",
            &[TemplateArg {
                name: "percent".into(),
                value: 15.0,
            }],
        )
        .unwrap();
        assert_eq!(r.title, "Battery low");
        assert_eq!(r.body, "Battery's at 15 percent and you're not plugged in.");

        // 5. thermal.hot
        let r = render("thermal.hot", &[]).unwrap();
        assert_eq!(r.title, "Running hot");
        assert_eq!(r.body, "Your laptop's been running hot for a while.");

        // 6. memory.pressure
        let r = render("memory.pressure", &[]).unwrap();
        assert_eq!(r.title, "Memory nearly full");
        assert_eq!(r.body, "Memory is nearly full, so things may feel slow.");

        // 7. session.long
        let r = render(
            "session.long",
            &[TemplateArg {
                name: "minutes".into(),
                value: 45.0,
            }],
        )
        .unwrap();
        assert_eq!(r.title, "Time for a break?");
        assert_eq!(
            r.body,
            "You've been at it for 45 minutes. Might be time for a short break."
        );

        // 8. part.degraded
        let r = render("part.degraded", &[]).unwrap();
        assert_eq!(r.title, "OctoPlus degraded");
        assert_eq!(
            r.body,
            "Part of OctoPlus isn't working right now, so it may miss things."
        );

        // 9. cleanup.done
        let r = render(
            "cleanup.done",
            &[TemplateArg {
                name: "freed_gib".into(),
                value: 4.2,
            }],
        )
        .unwrap();
        assert_eq!(r.title, "Cleanup finished");
        assert_eq!(r.body, "Freed 4.2 GiB.");
    }

    #[test]
    fn nudge_templates_render_and_merge_count_is_validated() {
        let r = render(
            "nudge.while_away",
            &[TemplateArg {
                name: "count".into(),
                value: 3.0,
            }],
        )
        .unwrap();
        assert_eq!(r.title, "While you were away");
        assert_eq!(
            r.body,
            "3 things needed your attention while you were away or busy."
        );
        let t = render("nudge.test", &[]).unwrap();
        assert_eq!(t.title, "Test nudge");
        // Free text can never ride in: the argument is a number and the name must match.
        assert_eq!(
            render("nudge.while_away", &[]),
            Err(NotifyError::BadArguments)
        );
        assert_eq!(
            render(
                "nudge.while_away",
                &[TemplateArg {
                    name: "count".into(),
                    value: f64::NAN
                }]
            ),
            Err(NotifyError::BadValue)
        );
    }

    #[test]
    fn test_render_validation_and_errors() {
        // Unknown template id
        assert_eq!(
            render("nonexistent.template", &[]),
            Err(NotifyError::UnknownTemplate)
        );

        // Extra argument
        assert_eq!(
            render(
                "thermal.hot",
                &[TemplateArg {
                    name: "extra".into(),
                    value: 1.0,
                }],
            ),
            Err(NotifyError::BadArguments)
        );

        // Missing argument
        assert_eq!(
            render("disk.nearly_full", &[]),
            Err(NotifyError::BadArguments)
        );

        // Wrong argument name
        assert_eq!(
            render(
                "disk.nearly_full",
                &[TemplateArg {
                    name: "wrong_name".into(),
                    value: 50.0,
                }],
            ),
            Err(NotifyError::BadArguments)
        );

        // Duplicate argument name
        assert_eq!(
            render(
                "session.long",
                &[
                    TemplateArg {
                        name: "minutes".into(),
                        value: 30.0,
                    },
                    TemplateArg {
                        name: "minutes".into(),
                        value: 60.0,
                    },
                ],
            ),
            Err(NotifyError::BadArguments)
        );

        // Non-finite value: NaN
        assert_eq!(
            render(
                "disk.nearly_full",
                &[TemplateArg {
                    name: "percent".into(),
                    value: f64::NAN,
                }],
            ),
            Err(NotifyError::BadValue)
        );

        // Non-finite value: Infinity
        assert_eq!(
            render(
                "disk.nearly_full",
                &[TemplateArg {
                    name: "percent".into(),
                    value: f64::INFINITY,
                }],
            ),
            Err(NotifyError::BadValue)
        );

        // Out of range: > 1e12
        assert_eq!(
            render(
                "disk.nearly_full",
                &[TemplateArg {
                    name: "percent".into(),
                    value: 1e13,
                }],
            ),
            Err(NotifyError::BadValue)
        );

        // Out of range: < -1e12
        assert_eq!(
            render(
                "disk.nearly_full",
                &[TemplateArg {
                    name: "percent".into(),
                    value: -1e13,
                }],
            ),
            Err(NotifyError::BadValue)
        );
    }

    #[derive(Clone, Default)]
    struct FakeNotificationServer {
        calls: Arc<Mutex<Vec<(String, String, String)>>>,
    }

    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl FakeNotificationServer {
        #[allow(clippy::too_many_arguments)]
        async fn notify(
            &self,
            app_name: String,
            _replaces_id: u32,
            _app_icon: String,
            summary: String,
            body: String,
            _actions: Vec<String>,
            hints: HashMap<String, zbus::OwnedVariant>,
            expire_timeout: i32,
        ) -> u32 {
            assert_eq!(app_name, "OctoPlus");
            assert_eq!(expire_timeout, -1);
            assert_eq!(
                hints.get("urgency").and_then(|v| u8::try_from(v).ok()),
                Some(1u8)
            );
            assert_eq!(
                hints
                    .get("desktop-entry")
                    .and_then(|v| <&str>::try_from(v).ok()),
                Some("org.octoplus.Confirm")
            );
            self.calls.lock().await.push((app_name, summary, body));
            101
        }
    }

    #[tokio::test]
    async fn test_p2p_notifier_delivery() {
        let (client_stream, server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = FakeNotificationServer::default();
        let calls = server.calls.clone();

        let guid = zbus::Guid::generate();
        let (_server_conn, client_conn) = tokio::try_join!(
            zbus::connection::Builder::unix_stream(server_stream)
                .server(guid)
                .p2p()
                .serve_at("/org/freedesktop/Notifications", server)
                .build(),
            zbus::connection::Builder::unix_stream(client_stream)
                .p2p()
                .build()
        )
        .unwrap();

        let notifier = Notifier::with_connection(client_conn);
        let rendered = Rendered {
            title: "Disk nearly full".into(),
            body: "Your disk is 92 percent full.".into(),
        };

        let id = notifier.deliver(&rendered).await.unwrap();
        assert_eq!(id, 101);

        let recorded = calls.lock().await;
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "OctoPlus");
        assert_eq!(recorded[0].1, "Disk nearly full");
        assert_eq!(recorded[0].2, "Your disk is 92 percent full.");
    }
}
