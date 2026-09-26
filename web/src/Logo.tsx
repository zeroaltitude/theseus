// The ship. Inline SVG so it renders identically everywhere, no emoji font
// needed. A hull, a mast, two sails, a waterline; the sails take the accent.
export default function Logo({ size = 22 }: { size?: number }) {
  return (
    <svg className="logo" width={size} height={size} viewBox="0 0 32 32" aria-label="Theseus" role="img">
      <path d="M15 4v17" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      <path d="M15 5 L26 19 H15 Z" fill="var(--accent)" />
      <path d="M13.6 8 L6 19 H13.6 Z" fill="var(--accent)" opacity=".6" />
      <path d="M3 22 H29 L25 27 H7 Z" fill="currentColor" />
      <path d="M2 29.5 q4 -2 8 0 t8 0 t8 0 t4 0" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" opacity=".55" />
    </svg>
  )
}
