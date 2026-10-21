//! 公共 broker 的登录凭据，随包发布。链路是端到端加密的，broker 只见密文，
//! 这对凭据泄露最多让人连上 broker，读不到任何房间内容。换成自己的 broker 只改这一个文件。

pub const HOST: &str = "b890fa3a.ala.cn-shenzhen.emqxsl.cn";
pub const SCHEME: &str = "mqtts";
pub const PORT_TLS: u16 = 8883;
pub const PORT_WSS: u16 = 8084;
pub const USERNAME: &str = "clipsync";
pub const PASSWORD: &str = "0ae0cf6e4016414fc46ee11c2ec4f2e3";

pub fn endpoint() -> String {
    format!("{SCHEME}://{HOST}:{PORT_TLS}")
}
