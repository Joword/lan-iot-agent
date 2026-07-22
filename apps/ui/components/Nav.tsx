"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const links = [
  { href: "/", label: "Home" },
  { href: "/devices", label: "Devices" },
  { href: "/companions", label: "Companions" },
  { href: "/scenes", label: "Scenes" },
  { href: "/chat", label: "Chat" },
  { href: "/pair", label: "Pair" },
] as const;

export function Nav() {
  const pathname = usePathname();

  return (
    <header className="border-b border-[var(--border)] bg-[var(--bg)]/80 backdrop-blur-sm">
      <nav className="mx-auto flex max-w-2xl items-center justify-between gap-4 px-6 py-3">
        <Link
          href="/"
          className="text-sm font-semibold tracking-tight text-[var(--fg)]"
        >
          LanIoT Agent
        </Link>
        <ul className="flex items-center gap-1 text-sm">
          {links.map(({ href, label }) => {
            const active =
              href === "/"
                ? pathname === "/"
                : pathname === href || pathname.startsWith(`${href}/`);
            return (
              <li key={href}>
                <Link
                  href={href}
                  className={
                    active
                      ? "rounded px-2.5 py-1.5 font-medium text-[var(--fg)]"
                      : "rounded px-2.5 py-1.5 text-[var(--muted)] hover:text-[var(--fg)]"
                  }
                  aria-current={active ? "page" : undefined}
                >
                  {label}
                </Link>
              </li>
            );
          })}
        </ul>
      </nav>
    </header>
  );
}
