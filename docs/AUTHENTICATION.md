# JWT 鉴权与密钥轮换

`JwtService` 签发和验证 HS256 访问令牌，受保护接口通过原有认证用例与提取器解析当前用户。
签名、issuer、audience、`exp` 验证失败统一返回 `401`，文案为
`Unauthorized: access token is invalid or expired`，不返回密钥、完整令牌或底层解析错误。
开发登录仅在 debug 构建提供；本功能不增加生产登录、refresh/logout 或逐会话撤销。

## 单密钥兼容

未配置 `JWT_KEY_SET`（或值为空）时继续使用 `JWT_SECRET`，签发不带 `kid`，原有令牌和
验证行为保持兼容。密钥至少 32 字节；生产环境禁止使用 `.env-example` 中的示例密钥。
`JWT_ISSUER`、`JWT_AUDIENCE` 与 `JWT_ACCESS_TOKEN_TTL_MINUTES` 在两种模式下共用。

`JwtConfig` 是公开 Rust 结构体，新增了 `key_set: Option<JwtKeySet>`；使用结构体字面量的
调用者需要补上 `key_set: None`。轮换调用者使用同一配置模块导出的 `JwtKeySet` 和
`JwtKeyConfig`，并将 `secret` 设为空字符串。`JwtService::new` 的调用签名不变。

## 轮换配置

`JWT_KEY_SET` 是一个 JSON 对象：

```json
{
  "current_kid": "B",
  "legacy_kid": "A",
  "keys": [
    {"kid": "B", "secret": "replace-with-the-new-random-secret"},
    {"kid": "A", "secret": "replace-with-the-old-random-secret", "accept_until": 1893456000}
  ]
}
```

上述密钥仅表示字段位置，实际部署必须替换为自己的随机密钥。`accept_until` 为 UTC Unix
整数秒，示例时间为 2030-01-01 00:00:00 UTC；应按实际部署时间与令牌剩余寿命设置。

- `keys` 必须包含 1–4 项，`kid` 唯一，由 1–64 个 ASCII 字母、数字、`-`、`_`、`.` 组成。
- `current_kid` 必须指向其中一项。只有该项用于签发，新令牌的 header 写入相应 `kid`。
- 每个非当前项都必须设置 `accept_until`，包括预分发的新密钥；切换当前项时重新发布配置快照。
- `legacy_kid` 可省略。配置后，无 `kid` 的旧令牌只使用这一项验证；该项必须有截止时间，
  即使它同时是当前签发项。未知 `kid` 不走 legacy，也不依次尝试其他密钥。
- 到达截止时间（`now >= accept_until`）即拒绝，独立于令牌 `exp` 和默认 60 秒过期宽限。
  当前项如设置截止时间，到点后签发也会失败；应在此之前切换至新的当前项。
- 截止时间必须是可表示的正 Unix 秒数；已过去的截止时间允许加载，但该项不再被使用。
- 每项密钥遵守原有长度和生产示例限制。重复项、未知 JSON 字段、缺少当前项、缺少必要
  截止时间、错误类型及非法时间都会在 `AppConfig::from_env` 阶段失败。

**启用前必须删除或清空进程环境和 `.env` 中的 `JWT_SECRET`，包括模板自带的默认值。**
非空 `JWT_SECRET` 与非空 `JWT_KEY_SET` 并存会拒绝启动，不会按优先级静默选择。
轮换 JSON 的解析错误与配置的 Debug 输出均隐藏密钥；启动快照只报告模式和集合数量。
写入 `.env` 时用单引号包住整段 JSON，保留 JSON 内的双引号（见 `.env-example`）。
模板中的轮换示例与 `JWT_SECRET` 使用相同示例密钥，`APP_ENV=production` 下均被拒绝。

## 准备、切换、退役

下面用 A 表示原密钥、B 表示新密钥，所有验证实例保持相同 issuer/audience。先选择 A 的
退役时间 `T_A`，覆盖预期部署时间、旧令牌剩余有效期及可接受的时钟偏差；准备阶段也为 B
设置明确的预分发接受截止 `T_B`。两个截止时间都必须晚于计划完成切换的时刻。

| 阶段 | 当前项 | 额外验证项 | legacy 映射 | 生效行为 |
| --- | --- | --- | --- | --- |
| 准备 | A（截止 `T_A`） | B（截止 `T_B`） | A | 签发带 A 标识的令牌；接受原无标识 A 令牌及后续 B 令牌 |
| 切换 | B（不设置截止） | A（截止 `T_A`） | A | 只用 B 签发；在 `T_A` 前接受合法 A/B 令牌 |
| 退役 | B | 无 | 无 | A 被移除；旧有 A 令牌被拒绝，B 继续通过 |

可用以下命令生成三个快照（需要 Python 3；不会打印密钥）。在已安全提供原 `JWT_SECRET`
的 shell 中执行，输出文件包含秘密，权限设为仅本人可读：

```bash
umask 077
export JWT_ROTATION_A="$JWT_SECRET"
export JWT_ROTATION_B="$(openssl rand -hex 32)"
export JWT_ROTATION_DIR="$(mktemp -d)"
python3 - <<'PY'
import json, os, time
from pathlib import Path

a, b = os.environ["JWT_ROTATION_A"], os.environ["JWT_ROTATION_B"]
assert len(a.encode()) >= 32 and len(b.encode()) >= 32
cutoff = int(time.time()) + 24 * 60 * 60  # 按实际部署和令牌寿命调整
snapshots = {
    "prepare": {"current_kid": "A", "legacy_kid": "A", "keys": [
        {"kid": "A", "secret": a, "accept_until": cutoff},
        {"kid": "B", "secret": b, "accept_until": cutoff}]},
    "switch": {"current_kid": "B", "legacy_kid": "A", "keys": [
        {"kid": "B", "secret": b},
        {"kid": "A", "secret": a, "accept_until": cutoff}]},
    "retire": {"current_kid": "B", "keys": [{"kid": "B", "secret": b}]},
}
for phase, config in snapshots.items():
    (Path(os.environ["JWT_ROTATION_DIR"]) / f"jwt-{phase}.json").write_text(json.dumps(config))
PY
unset JWT_ROTATION_A JWT_ROTATION_B JWT_SECRET
# 同时删除/清空 .env 中的 JWT_SECRET；按阶段选择快照启动进程：
export JWT_KEY_SET="$(cat "$JWT_ROTATION_DIR/jwt-prepare.json")"
cargo run --release
```

先让**所有验证实例**加载准备快照，再逐个加载切换快照；确认旧令牌不再需要后，或到达
`T_A` 后，加载退役快照并安全清理含 A 的秘密文件。每次启动读取一次配置，无热加载、
自动调度或集群配置分发。修改环境变量或删除磁盘文件不会改变正在运行进程的快照。

尚未完成准备的单密钥实例不能验证 B；尚未更新的实例不会因为别处移除 A 就立即撤销 A。
实例时钟、部署顺序和窗口长度会影响连续性，本地多配置测试不代表真实集群零中断验证。
发生密钥泄露时应立即从所有相关实例撤销泄露密钥，不必等待计划截止时间。
