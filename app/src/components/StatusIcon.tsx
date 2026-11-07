import {
  CheckCircleOutlined,
  ClockCircleOutlined,
  CalendarOutlined,
  UserOutlined,
  LockOutlined,
  ToolOutlined,
  QuestionCircleOutlined,
} from '@ant-design/icons'
import type { ReactNode } from 'react'

/**
 * 房态图标注册表。
 *
 * 数据库里只存图标名（`room_status.icon`），前端按名字映射到组件，
 * 这样改图标不用迁移数据；未知名字退化成问号图标。
 */
export const STATUS_ICONS: Record<string, ReactNode> = {
  CheckCircle: <CheckCircleOutlined />,
  ClockCircle: <ClockCircleOutlined />,
  Calendar: <CalendarOutlined />,
  User: <UserOutlined />,
  Lock: <LockOutlined />,
  Tool: <ToolOutlined />,
  QuestionCircle: <QuestionCircleOutlined />,
}

/** 设置页可选的图标（名字 + 预览）。 */
export const ICON_CHOICES = Object.keys(STATUS_ICONS)

export function statusIcon(name: string): ReactNode {
  return STATUS_ICONS[name] ?? STATUS_ICONS.QuestionCircle
}
