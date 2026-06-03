import React from "react";
import { BookOpen, Code2, Github, Home } from "lucide-react";

import { DemoPage } from "./pages/DemoPage";
import { DocsPage } from "./pages/DocsPage";
import { HomePage } from "./pages/HomePage";

type RouteId = "home" | "docs" | "demos";

interface NavItem {
  route: RouteId;
  path: string;
  label: string;
  icon: React.ReactElement;
}

const NAV_ITEMS: NavItem[] = [
  { route: "home", path: "/", label: "Home", icon: <Home size={16} aria-hidden="true" /> },
  { route: "docs", path: "/docs", label: "Docs", icon: <BookOpen size={16} aria-hidden="true" /> },
  { route: "demos", path: "/demos", label: "Demos", icon: <Code2 size={16} aria-hidden="true" /> },
];

export function App(): React.ReactElement {
  const [pathname, setPathname] = React.useState<string>(() => window.location.pathname);
  const route = routeFromPathname(pathname);

  React.useEffect(() => {
    const handlePopState = () => setPathname(window.location.pathname);
    window.addEventListener("popstate", handlePopState);
    return () => window.removeEventListener("popstate", handlePopState);
  }, []);

  const navigate = React.useCallback((path: string, event?: React.MouseEvent<HTMLAnchorElement>) => {
    event?.preventDefault();
    const href = routeHref(path);
    const targetPath = new URL(href, window.location.origin).pathname;
    if (window.location.pathname !== targetPath) {
      window.history.pushState(null, "", href);
    }
    setPathname(targetPath);
    window.scrollTo({ top: 0 });
  }, []);

  return (
    <div className={`site-shell site-route-${route}`}>
      <header className="site-header">
        <a className="site-brand" href={routeHref("/")} onClick={(event) => navigate("/", event)}>
          <span className="site-brand-mark">PDL</span>
          <span>
            <strong>PDL</strong>
            <small>Pipeline Data Language</small>
          </span>
        </a>
        <nav className="site-nav" aria-label="Primary">
          {NAV_ITEMS.map((item) => (
            <a
              aria-current={route === item.route ? "page" : undefined}
              className={`site-nav-link ${route === item.route ? "site-nav-link-active" : ""}`}
              href={routeHref(item.path)}
              key={item.route}
              onClick={(event) => navigate(item.path, event)}
            >
              {item.icon}
              {item.label}
            </a>
          ))}
        </nav>
        <a className="site-header-cta" href="https://github.com/williamcotton/pdl">
          <Github size={15} aria-hidden="true" />
          GitHub
        </a>
      </header>

      <main className="site-main">
        {route === "home" ? <HomePage navigate={navigate} routeHref={routeHref} /> : null}
        {route === "docs" ? (
          <DocsPage navigate={navigate} routeHref={routeHref} slug={docsSlugFromPathname(pathname)} />
        ) : null}
        {route === "demos" ? <DemoPage /> : null}
      </main>

      <footer className="site-footer">
        <span>PDL ships parser, analyzer, executor, LSP, CLI, and browser runtime together.</span>
        <div className="site-footer-links">
          <a href="https://github.com/williamcotton/pdl">GitHub</a>
          <a href={routeHref("/docs")} onClick={(event) => navigate("/docs", event)}>
            Documentation
          </a>
        </div>
      </footer>
    </div>
  );
}

function routeFromPathname(pathname: string): RouteId {
  const basePath = normalizedBasePath();
  let path = pathname;
  if (basePath && (path === basePath || path.startsWith(`${basePath}/`))) {
    path = path.slice(basePath.length) || "/";
  }
  const normalizedPath = path.length > 1 ? path.replace(/\/+$/, "") : path;
  if (normalizedPath === "/docs" || normalizedPath.startsWith("/docs/")) return "docs";
  if (normalizedPath === "/demos") return "demos";
  return "home";
}

function docsSlugFromPathname(pathname: string): string {
  const basePath = normalizedBasePath();
  let path = pathname;
  if (basePath && (path === basePath || path.startsWith(`${basePath}/`))) {
    path = path.slice(basePath.length) || "/";
  }
  const normalizedPath = path.length > 1 ? path.replace(/\/+$/, "") : path;
  if (normalizedPath === "/docs" || normalizedPath === "/") return "";
  if (normalizedPath.startsWith("/docs/")) return normalizedPath.slice("/docs/".length);
  return "";
}

function routeHref(path: string): string {
  const basePath = normalizedBasePath();
  if (!basePath) {
    return path;
  }
  return path === "/" ? `${basePath}/` : `${basePath}${path}`;
}

function normalizedBasePath(): string {
  const base = import.meta.env.BASE_URL || "/";
  if (base === "/") {
    return "";
  }
  return base.replace(/\/+$/, "");
}
