import React from "react";
import { ArrowLeft, ArrowRight } from "lucide-react";

import { DOC_TOPICS, type DocTopic, topicForSlug } from "./docs/content";
import { LiveExample } from "./docs/LiveExample";

interface DocsPageProps {
  slug: string;
  navigate: (path: string, event?: React.MouseEvent<HTMLAnchorElement>) => void;
  routeHref: (path: string) => string;
}

function docPath(slug: string): string {
  return slug ? `/docs/${slug}` : "/docs";
}

export function DocsPage({ slug, navigate, routeHref }: DocsPageProps): React.ReactElement {
  const topic = topicForSlug(slug);
  const index = DOC_TOPICS.findIndex((entry) => entry.slug === topic.slug);
  const previous = index > 0 ? DOC_TOPICS[index - 1] : null;
  const next = index < DOC_TOPICS.length - 1 ? DOC_TOPICS[index + 1] : null;

  return (
    <div className="docs-page docs-layout">
      <aside className="docs-sidebar" aria-label="Documentation">
        <p className="eyebrow">Documentation</p>
        <nav className="docs-sidebar-nav">
          {DOC_TOPICS.map((entry) => {
            const active = entry.slug === topic.slug;
            return (
              <a
                aria-current={active ? "page" : undefined}
                className={`docs-sidebar-link ${active ? "docs-sidebar-link-active" : ""}`}
                href={routeHref(docPath(entry.slug))}
                key={entry.slug || "overview"}
                onClick={(event) => navigate(docPath(entry.slug), event)}
              >
                {entry.nav}
              </a>
            );
          })}
        </nav>
      </aside>

      <article className="docs-content">
        <header className="docs-content-head">
          <h1>{topic.title}</h1>
          <p className="docs-lede">{topic.lede}</p>
        </header>

        {topic.sections.map((section) => (
          <section className="docs-topic-section" id={section.id} key={section.id}>
            <h2>{section.title}</h2>
            <div className="docs-prose">{section.body}</div>
            {section.example ? (
              <LiveExample
                files={section.example.files}
                id={section.example.id}
                key={section.example.id}
                source={section.example.source}
                stdoutFormat={section.example.stdoutFormat}
              />
            ) : null}
          </section>
        ))}

        <nav className="docs-pager" aria-label="Documentation pages">
          {previous ? (
            <DocsPagerLink direction="prev" navigate={navigate} routeHref={routeHref} topic={previous} />
          ) : (
            <span />
          )}
          {next ? (
            <DocsPagerLink direction="next" navigate={navigate} routeHref={routeHref} topic={next} />
          ) : (
            <span />
          )}
        </nav>
      </article>
    </div>
  );
}

function DocsPagerLink({
  direction,
  topic,
  navigate,
  routeHref,
}: {
  direction: "prev" | "next";
  topic: DocTopic;
  navigate: (path: string, event?: React.MouseEvent<HTMLAnchorElement>) => void;
  routeHref: (path: string) => string;
}): React.ReactElement {
  const path = docPath(topic.slug);
  return (
    <a
      className={`docs-pager-link docs-pager-${direction}`}
      href={routeHref(path)}
      onClick={(event) => navigate(path, event)}
    >
      {direction === "prev" ? <ArrowLeft size={16} aria-hidden="true" /> : null}
      <span>
        <small>{direction === "prev" ? "Previous" : "Next"}</small>
        <strong>{topic.nav}</strong>
      </span>
      {direction === "next" ? <ArrowRight size={16} aria-hidden="true" /> : null}
    </a>
  );
}
