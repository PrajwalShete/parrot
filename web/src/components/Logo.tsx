/** The Parrot mark: a ring with voice bars. The bars move while `live`. */
export function Logo({ size = 20, live = false }: { size?: number; live?: boolean }) {
  return (
    <svg className={`logo ${live ? "is-live" : ""}`} width={size} height={size} viewBox="0 0 32 32" aria-hidden>
      <circle cx="16" cy="16" r="13" fill="none" stroke="currentColor" strokeWidth="3" />
      <rect className="logo-bar b1" x="10" y="12" width="2.6" height="8" rx="1.3" />
      <rect className="logo-bar b2" x="14.7" y="9" width="2.6" height="14" rx="1.3" />
      <rect className="logo-bar b3" x="19.4" y="12.5" width="2.6" height="7" rx="1.3" />
    </svg>
  );
}
