//! SSH Agent 转发支持。

/// 管理会话的 SSH agent 转发。
///
/// Agent 转发允许远程主机使用本地 SSH agent 进行身份验证,
/// 从而能够在服务器之间跳转而无需复制密钥。
pub struct AgentForwardManager {
    /// 是否已请求 agent 转发。
    requested: bool,
    /// 服务器是否确认了 agent 转发。
    active: bool,
}

impl AgentForwardManager {
    pub fn new() -> Self {
        Self {
            requested: false,
            active: false,
        }
    }

    /// 在通道上请求 agent 转发。
    /// 实际请求通过 `channel.request_agent_forward()` 发送。
    pub fn request(&mut self) {
        self.requested = true;
    }

    /// 标记 agent 转发已被服务器确认。
    pub fn confirm(&mut self) {
        self.active = true;
        log::info!("SSH agent forwarding confirmed");
    }

    /// 标记 agent 转发已被服务器拒绝。
    pub fn deny(&mut self) {
        self.active = false;
        log::warn!("SSH agent forwarding denied by server");
    }

    pub fn is_requested(&self) -> bool {
        self.requested
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl Default for AgentForwardManager {
    fn default() -> Self {
        Self::new()
    }
}
