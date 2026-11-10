//! Core、普通设置界面和独立安装进度窗口共用的更新状态契约。
//!
//! Java 读者可把 Phase + Progress 看作枚举状态与响应 DTO；它们会序列化为控制协议
//! 的 JSON，也会写入安装结果文件。它们不包含安装 Plan，不能仅凭 Ready 状态反序列化
//! 出一个可执行的安装任务；当前进程仍需持有已经验证、暂存完成的 prepared plan。
use serde::{Deserialize, Serialize};

/// 区分准备、等待和实际替换，调用方不能只用一个“正在更新”布尔量决定可否取消。
///
/// Ready 仍占用本次更新任务，等待用户再次确认；Waiting 正在排空代理请求，可退回
/// Ready；Installing 已进入 helper 交接流程，普通取消命令会拒绝。Complete 表示
/// helper 已核对新版服务，Failed 与 RecoveryFailed 则必须结合 error/日志判断：
/// 后者表示自动恢复未能确认，不能用“关闭提示”冒充恢复成功。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Downloading,
    Verifying,
    Preparing,
    Ready,
    Waiting,
    Installing,
    Complete,
    Cancelled,
    Failed,
    RecoveryFailed,
}

/// 某一时刻的状态快照，不是可重放的任务或跨进程事务日志。
///
/// downloaded/total 只描述安装包下载字节，不能当作安装整体完成率；backup_path 是
/// 本机恢复入口，logs 有界。Core 命令回复、后续轮询及 helper 写入文件可能处于不同
/// 时刻，界面需继续读取终态，不能把 update_start 返回成功当成安装已完成。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Progress {
    pub phase: Phase,
    pub tag: String,
    pub downloaded: u64,
    pub total: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub logs: Vec<String>,
    #[serde(default)]
    pub backup_path: Option<String>,
}
impl Progress {
    /// “占用更新流程”而非“CPU 正在工作”：Ready 等待确认时也为 true，因此不能
    /// 再 start 另一任务或 acknowledge 清空它。取消行为由 Updater::cancel 按具体
    /// phase 判断，这个聚合布尔值不是统一的可取消判据。
    pub fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Downloading
                | Phase::Verifying
                | Phase::Preparing
                | Phase::Ready
                | Phase::Waiting
                | Phase::Installing
        )
    }
}
