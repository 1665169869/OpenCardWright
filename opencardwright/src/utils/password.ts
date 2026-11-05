/**
 * 密码规则。与后端 `src-tauri/src/auth.rs` 的 `PASSWORD_RULES` / `validate_password` 一一对应，
 * 改一处必须改另一处，否则前端放行、后端拒绝会很难解释。
 */

export const MIN_PASSWORD_LEN = 8
export const MAX_PASSWORD_LEN = 128

export const PASSWORD_RULES: string[] = [
  `长度 ${MIN_PASSWORD_LEN}–${MAX_PASSWORD_LEN} 位`,
  '必须同时包含字母和数字',
  '不能与登录名相同',
  '只保存 Argon2id 哈希，忘记只能由管理员重置',
]

/** 返回所有不满足的规则；空数组表示通过。 */
export function passwordIssues(password: string, login: string): string[] {
  const issues: string[] = []
  if (password.length < MIN_PASSWORD_LEN) {
    issues.push(`长度不足，至少 ${MIN_PASSWORD_LEN} 位`)
  }
  if (password.length > MAX_PASSWORD_LEN) {
    issues.push(`长度超限，最多 ${MAX_PASSWORD_LEN} 位`)
  }
  if (!/[A-Za-z]/.test(password)) {
    issues.push('缺少字母')
  }
  if (!/[0-9]/.test(password)) {
    issues.push('缺少数字')
  }
  const name = login.trim()
  if (name && password.toLowerCase() === name.toLowerCase()) {
    issues.push('不能与登录名相同')
  }
  return issues
}

/** 两次输入是否一致。 */
export function confirmIssue(password: string, confirm: string): string | null {
  if (!confirm) return '请再输入一次密码'
  if (password !== confirm) return '两次输入的密码不一致'
  return null
}
