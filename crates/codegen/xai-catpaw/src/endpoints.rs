//! CatPaw upstream endpoint paths.

pub struct ApiPaths;

impl ApiPaths {
    // Passport/login family.
    pub const LOGIN_QRCODE: &str = "/api/login/qrcode";
    pub const LOGIN_ACCESS_TOKEN: &str = "/api/login/accessToken";
    pub const LOGIN_SEND_SMS: &str = "/api/login/sendSmsVerificationCode";
    pub const LOGIN_VERIFY_MOBILE: &str = "/api/login/mobile/verify";
    pub const LOGIN_MOBILE: &str = "/api/login/mobile";
    pub const LOGIN_BIND_MOBILE: &str = "/api/login/bindMobile";
    pub const LOGIN_REFRESH: &str = "/api/login/refreshToken";
    pub const LOGIN_USERINFO: &str = "/api/login/userInfo";
    pub const LOGOUT: &str = "/api/logout";

    // Encrypted GPT/chat family.
    pub const GPT_OPENAI_STREAM: &str = "/api/gpt/openai/stream";
    pub const GPT_CHAT_COMPLETIONS: &str = "/api/gpt/chat/completions";
    pub const GPT_MODEL_LIST: &str = "/api/chat/getModelTypeList";
    pub const CHAT_MODEL_USAGE: &str = "/api/chat/model/usage";
    pub const USER_LIMIT: &str = "/api/user/limit";
    pub const USER_ADD_QUOTA: &str = "/api/user/addQuota";

    // Plain JSON + SSE Remote Agent family.
    pub const AGENT_CONVERSATION_CREATE: &str = "/api/agent/conversation/create";
    pub const AGENT_CONVERSATION_CONTINUE: &str = "/api/agent/conversation/continue";
    pub const AGENT_STREAM_CONNECT: &str = "/api/agent/stream/connect";
}

#[cfg(test)]
mod tests {
    use super::ApiPaths;

    #[test]
    fn core_paths_are_stable() {
        assert_eq!(ApiPaths::LOGIN_QRCODE, "/api/login/qrcode");
        assert_eq!(ApiPaths::GPT_OPENAI_STREAM, "/api/gpt/openai/stream");
        assert_eq!(ApiPaths::AGENT_STREAM_CONNECT, "/api/agent/stream/connect");
    }
}
