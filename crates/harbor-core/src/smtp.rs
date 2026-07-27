//! SMTP send via XOAUTH2.

use native_tls::TlsConnector;

use crate::Provider;

#[derive(Debug, thiserror::Error)]
pub enum SmtpError {
    #[error("smtp: {0}")]
    Smtp(#[from] lettre::transport::smtp::Error),
    #[error("lettre: {0}")]
    Lettre(#[from] lettre::error::Error),
    #[error("tls: {0}")]
    Tls(#[from] native_tls::Error),
    #[error("account has no email address")]
    MissingEmail,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, SmtpError>;

/// A message to send.
#[derive(Debug, Clone)]
pub struct OutgoingMessage {
    pub from_email: String,
    pub from_name: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

pub fn smtp_host(provider: Provider) -> &'static str {
    match provider {
        Provider::Gmail => "smtp.gmail.com",
        Provider::Outlook => "smtp.office365.com",
    }
}

/// Send a message via SMTP STARTTLS with XOAUTH2.
pub fn send_message(
    provider: Provider,
    email: &str,
    access_token: &str,
    msg: &OutgoingMessage,
) -> Result<()> {
    use lettre::message::{header::ContentType, Mailbox, Message, MultiPart, SinglePart};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::Transport;

    if email.is_empty() {
        return Err(SmtpError::MissingEmail);
    }

    let host = smtp_host(provider);
    let port = 587;

    // Build the email.
    let from_mailbox = match &msg.from_name {
        Some(name) => Mailbox::new(Some(name.clone()), msg.from_email.parse().map_err(|e| {
            SmtpError::Other(format!("invalid from address: {e}"))
        })?),
        None => Mailbox::new(None, msg.from_email.parse().map_err(|e| {
            SmtpError::Other(format!("invalid from address: {e}"))
        })?),
    };

    let mut builder = Message::builder().from(from_mailbox);

    for addr in &msg.to {
        let mbox: Mailbox = addr.parse().map_err(|e| {
            SmtpError::Other(format!("invalid to address '{addr}': {e}"))
        })?;
        builder = builder.to(mbox);
    }
    for addr in &msg.cc {
        let mbox: Mailbox = addr.parse().map_err(|e| {
            SmtpError::Other(format!("invalid cc address '{addr}': {e}"))
        })?;
        builder = builder.to(mbox);
    }
    for addr in &msg.bcc {
        let mbox: Mailbox = addr.parse().map_err(|e| {
            SmtpError::Other(format!("invalid bcc address '{addr}': {e}"))
        })?;
        builder = builder.to(mbox);
    }

    builder = builder.subject(&msg.subject);

    if let Some(irt) = &msg.in_reply_to {
        builder = builder.header(lettre::message::header::InReplyTo::from(irt.clone()));
    }
    if let Some(refs) = &msg.references {
        builder = builder.header(lettre::message::header::References::from(refs.clone()));
    }

    // Message-ID
    let msg_id = format!(
        "<harbor-{}@{}>",
        uuid::Uuid::new_v4(),
        msg.from_email.split('@').nth(1).unwrap_or("local")
    );
    builder = builder.header(lettre::message::header::MessageId::from(msg_id));

    let built_email = if let Some(html) = &msg.body_html {
        builder.multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(msg.body_text.clone()),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html.clone()),
                ),
        )?
    } else {
        builder.singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_PLAIN)
                .body(msg.body_text.clone()),
        )?
    };

    // Connect with STARTTLS + XOAUTH2.
    let _tls = TlsConnector::builder().build()?;

    // XOAUTH2: user = email, password = access_token.
    let creds = Credentials::new(email.to_string(), access_token.to_string());

    let transport = lettre::SmtpTransport::starttls_relay(host)?
        .port(port)
        .credentials(creds)
        .build();

    transport.send(&built_email)?;
    Ok(())
}
