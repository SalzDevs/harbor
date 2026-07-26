/// SASL XOAUTH2 initial client response (unencoded; IMAP crate base64-encodes).
pub struct XOAuth2 {
    pub user: String,
    pub access_token: String,
}

impl imap::Authenticator for XOAuth2 {
    type Response = String;

    fn process(&self, _challenge: &[u8]) -> Self::Response {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.user, self.access_token
        )
    }
}
