import { Typography } from 'antd'
import { PASSWORD_RULES } from '../utils/password'

const { Text } = Typography

/**
 * 设置密码时把规则直接摆出来，而不是让人去猜为什么被拒。
 *
 * `issues` 有值时把未满足的项标红——只在用户点过提交后才传，避免一进页面就一片红。
 */
export default function PasswordRules({ issues = [] }: { issues?: string[] }) {
  return (
    <div style={{ marginTop: 2 }}>
      <Text type="secondary" style={{ fontSize: 12 }}>
        密码规则：
      </Text>
      <ul
        style={{
          margin: '2px 0 0',
          paddingLeft: 18,
          fontSize: 12,
          lineHeight: '18px',
          color: '#8c8c8c',
        }}
      >
        {PASSWORD_RULES.map((rule) => (
          <li key={rule}>{rule}</li>
        ))}
      </ul>
      {issues.length > 0 ? (
        <Text type="danger" style={{ fontSize: 12 }}>
          未满足：{issues.join('；')}
        </Text>
      ) : null}
    </div>
  )
}
