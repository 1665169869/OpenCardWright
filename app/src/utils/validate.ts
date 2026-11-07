/**
 * 文本类输入的前端校验。规则与后端 `commands.rs` 的 `require_text` / `require_login_name` /
 * `require_room_no` 对齐：前端先拦一道给出即时提示，后端仍然会再校验一次（防绕过）。
 */

/** 通用：非空 + 长度上限。`label` 用于拼提示语。 */
export function textIssue(value: string, label: string, min: number, max: number): string | null {
  const v = value.trim()
  if (v.length < min) return `请填写${label}`
  if ([...v].length > max) return `${label}最多 ${max} 个字符`
  return null
}

/** 房号：去掉首尾空白后 1–16 位，字母/数字/横线/下划线。 */
export function roomNoIssue(value: string): string | null {
  const v = value.trim()
  if (!v) return '请填写房号'
  if (v.length > 16) return '房号最多 16 个字符'
  if (!/^[A-Za-z0-9_-]+$/.test(v)) return '房号只能包含字母、数字、横线或下划线'
  return null
}

/** 登录账号：3–32 位，字母/数字/下划线/点/横线，且以字母或数字开头。 */
export function loginIssue(value: string): string | null {
  const v = value.trim()
  if (v.length < 3) return '登录账号至少 3 个字符'
  if (v.length > 32) return '登录账号最多 32 个字符'
  if (!/^[A-Za-z0-9][A-Za-z0-9_.-]*$/.test(v)) {
    return '只能含字母、数字、下划线、点或横线，且以字母或数字开头'
  }
  return null
}
