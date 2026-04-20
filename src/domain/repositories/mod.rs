/// 领域层只定义仓储抽象，不直接依赖 SeaORM。
///
/// 当前脚手架还没有绑定具体业务表，因此这里只保留仓储接口入口。
pub trait Repository: Send + Sync {}
