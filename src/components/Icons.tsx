interface IconProps {
  size?: number
  className?: string
  strokeWidth?: number
}

const base = (size = 16) => ({
  width: size,
  height: size,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const
})

export const IconClipboard = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <rect x="5" y="4" width="14" height="17" rx="2.5" />
    <path d="M9 4.5V3.5A1.5 1.5 0 0 1 10.5 2h3A1.5 1.5 0 0 1 15 3.5v1" />
    <path d="M9 11h6M9 15h4" />
  </svg>
)

export const IconImage = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <circle cx="8.5" cy="9.5" r="1.6" />
    <path d="M21 15.5l-4.5-4.5L7 20" />
  </svg>
)

export const IconVideo = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <rect x="2.5" y="5.5" width="13" height="13" rx="2.5" />
    <path d="M15.5 10.5l4-2.5v8l-4-2.5z" />
  </svg>
)

export const IconUpload = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M12 16V4M8 8l4-4 4 4" />
    <path d="M4 15v3.5A2.5 2.5 0 0 0 6.5 21h11A2.5 2.5 0 0 0 20 18.5V15" />
  </svg>
)

export const IconPlugin = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M9 3v3M15 3v3" />
    <path d="M6 6h12a1 1 0 0 1 1 1v4h-2.5a2.5 2.5 0 0 0 0 5H19v3a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1v-3H7.5a2.5 2.5 0 0 0 0-5H5V7a1 1 0 0 1 1-1z" />
  </svg>
)

export const IconCode = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M8.5 8.5L5 12l3.5 3.5" />
    <path d="M15.5 8.5L19 12l-3.5 3.5" />
    <path d="M13.2 5.5l-2.4 13" />
  </svg>
)

export const IconSettings = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-2.7 1.1V21a2 2 0 1 1-4 0v-.1A1.6 1.6 0 0 0 7.5 19.4a1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.6 1.6 0 0 0 3 14.5H3a2 2 0 1 1 0-4h.1A1.6 1.6 0 0 0 4.6 7.5a1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 2.7 1.1 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z" />
  </svg>
)

export const IconSearch = ({ size = 14, className, strokeWidth = 2 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <circle cx="11" cy="11" r="7" />
    <path d="M20 20l-3.5-3.5" />
  </svg>
)

export const IconPin = ({ size = 13, className, strokeWidth = 1.8 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M9 4h6l-1 6 3 3H7l3-3-1-6zM12 13v7" />
  </svg>
)

export const IconTrash = ({ size = 13, className, strokeWidth = 1.8 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13" />
  </svg>
)

export const IconCopy = ({ size = 13, className, strokeWidth = 1.8 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <rect x="9" y="9" width="11" height="11" rx="2" />
    <path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h3" />
  </svg>
)

export const IconMonitor = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <rect x="2.5" y="4" width="19" height="13" rx="2" />
    <path d="M8 21h8M12 17v4" />
  </svg>
)

export const IconScroll = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <rect x="6" y="2.5" width="12" height="19" rx="2" />
    <path d="M12 7v5M12 15.5h.01" />
  </svg>
)

export const IconX = ({ size = 14, className, strokeWidth = 1.9 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
)

export const IconMinus = ({ size = 14, className, strokeWidth = 1.9 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M5 12h14" />
  </svg>
)

export const IconSquare = ({ size = 12, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <rect x="4" y="4" width="16" height="16" rx="2" />
  </svg>
)

export const IconCheck = ({ size = 14, className, strokeWidth = 2.2 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M4 12.5l5 5L20 6.5" />
  </svg>
)

export const IconText = ({ size, className, strokeWidth = 1.8 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M5 6V4h14v2M12 4v16M9 20h6" />
  </svg>
)

export const IconSave = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M5 4h11l3 3v13H5z" />
    <path d="M8 4v5h7V4M8 20v-5h8v5" />
  </svg>
)

export const IconLoader = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} strokeWidth={2} className={`spin ${className ?? ''}`}>
    <path d="M12 3a9 9 0 1 0 9 9" />
  </svg>
)

export const IconLayers = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M12 3l9 5-9 5-9-5 9-5z" />
    <path d="M3 13l9 5 9-5" />
  </svg>
)

export const IconPlus = ({ size = 13, className, strokeWidth = 2 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M12 5v14M5 12h14" />
  </svg>
)

/** 手势：一条划出来的曲线 + 起点 */
export const IconGesture = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M4 17c2.5-8 5.5-10.5 8-7.5S15.5 19 20.5 10.5" strokeLinecap="round" />
    <circle cx="4" cy="17" r="1.4" />
  </svg>
)

export const IconRefresh = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M20 12a8 8 0 1 1-2.3-5.6" />
    <path d="M20 4v5h-5" />
  </svg>
)

export const IconFolder = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M3 7.5A1.5 1.5 0 0 1 4.5 6H9l2 2.5h8.5A1.5 1.5 0 0 1 21 10v8a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18z" />
  </svg>
)

export const IconHistory = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M3.5 12a8.5 8.5 0 1 0 2.8-6.3" />
    <path d="M3.5 4.5V10h5.5" />
    <path d="M12 8v4.5l3 1.8" />
  </svg>
)

/** hosts 页用：一张带"网络行"的纸 */
export const IconHosts = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M6 3h8l4 4v14H6z" />
    <path d="M14 3v4h4" />
    <path d="M9 12h6M9 15.5h6M9 19h3.5" />
  </svg>
)

/** 图标提取：一个应用方块 + 取图的对角箭头 */
export const IconApp = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <rect x="3" y="3" width="12" height="12" rx="2.5" />
    <path d="M15 15h4a2 2 0 0 1 2 2v2a2 2 0 0 1-2 2h-4" />
    <path d="M13 13l5-5" opacity="0.55" />
  </svg>
)

/** 选择工具：鼠标指针 */
export const IconPointer = ({ size, className, strokeWidth = 1.7 }: IconProps) => (
  <svg {...base(size)} strokeWidth={strokeWidth} className={className}>
    <path d="M5.5 3.2 18 11.4l-5.3 1.2-1.6 5.2z" />
  </svg>
)
