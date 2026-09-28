import { useState } from "react";
import type { ComponentType, ReactNode } from "react";

import {
  ArrowLeft,
  Code2,
  GitPullRequest,
  Globe,
  MessageSquare,
  Monitor,
  Moon,
  Palette,
  PanelRight,
  Sparkles,
  Sun,
} from "@/components/ui/icons";
import { ScrollArea } from "@/components/ui/scroll-area";

import { Dock } from "../../dock/Dock";
import type { DockTab } from "../../dock/Dock";
import { GitHubPullRequestPanel } from "../../git/GitHubPullRequestPanel";
import { PullRequestsPage } from "../../github/PullRequestsPage";
import { useLanguage } from "../../i18n";
import { useTheme } from "../../theme";
import { DesignSystemPreview } from "../DesignSystemPreview";
import {
  loadPullRequest,
  loadPullRequests,
  pullRequestPanelApi,
  pullRequestTasks,
} from "./fixtures";
import { SettingsScenario } from "./SettingsScenario";

import "./ui-lab.css";

type UiLabRoute =
  | "home"
  | "design-system"
  | "pull-requests"
  | "pr-dock"
  | "settings";
interface UiLabCard {
  route: UiLabRoute;
  icon: ComponentType<{ className?: string }>;
  title: string;
  description: string;
  tags: readonly string[];
  fixture?: boolean;
}

const catalogCards: readonly UiLabCard[] = [
  {
    route: "settings",
    icon: Palette,
    title: "Settings controls",
    description: "Production appearance, catalog search and editor sizing.",
    tags: ["Forms", "Overflow"],
    fixture: true,
  },
  {
    route: "design-system",
    icon: Palette,
    title: "Design system",
    description:
      "Tokens, type, spacing, controls, business primitives, states, and accessibility.",
    tags: ["Foundations", "Components", "States"],
  },
  {
    route: "pull-requests",
    icon: GitPullRequest,
    title: "PR workspace",
    description:
      "Production list, detail, changes, checks, task link, and inspector in one stable state.",
    tags: ["Master detail", "Inspector", "Responsive"],
    fixture: true,
  },
  {
    route: "pr-dock",
    icon: PanelRight,
    title: "Conversation PR Dock",
    description:
      "The production PR review panel beside a restrained conversation shell.",
    tags: ["Dock", "Review", "Diff"],
    fixture: true,
  },
];

const previewCards = [
  {
    href: "?rich-transcript=1",
    icon: MessageSquare,
    title: "Rich transcript",
    description:
      "Conversation content, tool calls, tables, and long-form rendering.",
  },
  {
    href: "?pet-preview=1",
    icon: Sparkles,
    title: "Pet animation",
    description: "Desktop pet sprites, states, direction, and motion preview.",
  },
] as const;

function routeFromLocation(): UiLabRoute {
  const value = new URLSearchParams(window.location.search).get("ui-lab");
  return value === "design-system" ||
    value === "pull-requests" ||
    value === "pr-dock" ||
    value === "settings"
    ? value
    : "home";
}

function uiLabHref(
  route: UiLabRoute,
  theme: "system" | "light" | "dark",
  locale: "en" | "zh-CN"
): string {
  const params = new URLSearchParams({
    "ui-lab": route,
    theme,
    lang: locale === "zh-CN" ? "zh" : "en",
  });
  return `?${params.toString()}`;
}

function CatalogCard({ card }: { card: UiLabCard }) {
  const { preference } = useTheme();
  const { locale } = useLanguage();
  const Icon = card.icon;
  return (
    <a className="ui-lab-card" href={uiLabHref(card.route, preference, locale)}>
      <span className="ui-lab-card-icon">
        <Icon className="size-4" />
      </span>
      <span className="ui-lab-card-heading">
        <strong>{card.title}</strong>
        <span aria-hidden="true">→</span>
      </span>
      <span className="ui-lab-card-description">{card.description}</span>
      <span className="ui-lab-card-footer">
        <span className="ui-lab-tags">
          {card.tags.map((tag) => (
            <span key={tag}>{tag}</span>
          ))}
        </span>
        {card.fixture === true ? (
          <span className="ui-lab-fixture-badge">Fixture</span>
        ) : null}
      </span>
    </a>
  );
}

function PreviewCard({ card }: { card: (typeof previewCards)[number] }) {
  const Icon = card.icon;
  return (
    <a className="ui-lab-preview-card" href={card.href}>
      <Icon className="size-4" />
      <span>
        <strong>{card.title}</strong>
        <small>{card.description}</small>
      </span>
      <span aria-hidden="true">↗</span>
    </a>
  );
}

function ThemeLinks({ route }: { route: UiLabRoute }) {
  const { preference } = useTheme();
  const { locale } = useLanguage();
  const items = [
    { id: "system", label: "System", icon: Monitor },
    { id: "light", label: "Light", icon: Sun },
    { id: "dark", label: "Dark", icon: Moon },
  ] as const;
  return (
    <div className="ui-lab-control-group" aria-label="Preview theme">
      {items.map(({ id, label, icon: Icon }) => (
        <a
          key={id}
          aria-current={preference === id ? "true" : undefined}
          aria-label={`${label} theme`}
          href={uiLabHref(route, id, locale)}
          title={`${label} theme`}
        >
          <Icon className="size-3.5" />
        </a>
      ))}
    </div>
  );
}

function LocaleLinks({ route }: { route: UiLabRoute }) {
  const { preference } = useTheme();
  const { locale } = useLanguage();
  return (
    <div
      className="ui-lab-control-group ui-lab-locale-links"
      aria-label="Preview language"
    >
      <Globe className="size-3.5" aria-hidden="true" />
      <a
        aria-current={locale === "en" ? "true" : undefined}
        href={uiLabHref(route, preference, "en")}
      >
        EN
      </a>
      <a
        aria-current={locale === "zh-CN" ? "true" : undefined}
        href={uiLabHref(route, preference, "zh-CN")}
      >
        中文
      </a>
    </div>
  );
}

function ScenarioToolbar({
  route,
  title,
}: {
  route: UiLabRoute;
  title: string;
}) {
  const { preference } = useTheme();
  const { locale } = useLanguage();
  return (
    <header className="ui-lab-scenario-toolbar">
      <a className="ui-lab-back" href={uiLabHref("home", preference, locale)}>
        <ArrowLeft className="size-3.5" />
        <span>UI Lab</span>
      </a>
      <span className="ui-lab-scenario-title">{title}</span>
      <span className="ui-lab-scenario-spacer" />
      <span className="ui-lab-fixture-badge">Deterministic fixture</span>
      <span className="ui-lab-dev-badge">Dev only</span>
      <ThemeLinks route={route} />
      <LocaleLinks route={route} />
    </header>
  );
}

function ScenarioShell({
  children,
  route,
  title,
}: {
  children: ReactNode;
  route: UiLabRoute;
  title: string;
}) {
  return (
    <div className="ui-lab-scenario">
      <ScenarioToolbar route={route} title={title} />
      <div className="ui-lab-scenario-content">{children}</div>
    </div>
  );
}

function PullRequestsScenario() {
  return (
    <ScenarioShell route="pull-requests" title="PR workspace">
      <PullRequestsPage
        activeTaskId="ui-lab-pr-task"
        loadPullRequest={loadPullRequest}
        loadPullRequests={loadPullRequests}
        onChat={() => {
          /* empty */
        }}
        onOpenTask={() => {
          /* empty */
        }}
        tasks={pullRequestTasks}
      />
    </ScenarioShell>
  );
}

function ConversationFixture() {
  return (
    <main className="ui-lab-conversation" aria-label="Conversation fixture">
      <header>
        <div>
          <strong>Usage sidebar review</strong>
          <span>acme/code-two · feat/sidebar-usage-widget</span>
        </div>
      </header>
      <ScrollArea className="min-h-0 flex-1">
        <div className="ui-lab-transcript">
          <article className="ui-lab-message ui-lab-message-user">
            <span>YOU</span>
            <p>
              Can you review the usage widget changes and make sure the narrow
              layout still works?
            </p>
          </article>
          <article className="ui-lab-message ui-lab-message-assistant">
            <span>C2</span>
            <p>
              The implementation is ready for review. I verified the responsive
              shell and the three desktop checks pass.
            </p>
            <div className="ui-lab-run-summary">
              <Code2 className="size-3.5" />3 checks passed · 30 files changed
            </div>
          </article>
        </div>
      </ScrollArea>
      <div className="ui-lab-composer" aria-label="Message composer preview">
        <span>Ask a follow-up…</span>
        <kbd>⌘ ↵</kbd>
      </div>
    </main>
  );
}

function PullRequestDockScenario() {
  const [titlebarHost, setTitlebarHost] = useState<HTMLDivElement | null>(null);
  const [tab, setTab] = useState<DockTab>("pull-request");
  return (
    <ScenarioShell route="pr-dock" title="Conversation PR Dock">
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div ref={setTitlebarHost} className="flex shrink-0 justify-end" />
        <div className="ui-lab-dock-layout">
          <ConversationFixture />
          <Dock
            titlebarHost={titlebarHost}
            availableSurfaces={[
              "trajectory",
              "browser",
              "terminal",
              "side-chat",
              "files",
              "git",
              "pull-request",
            ]}
            content={{
              trajectory: <p className="p-4">Execution trajectory fixture</p>,
              browser: <p className="p-4">Browser fixture</p>,
              terminal: <p className="p-4">Terminal fixture</p>,
              "side-chat": <p className="p-4">Side chat fixture</p>,
              files: <p className="p-4">Files fixture</p>,
              git: <p className="p-4">Git fixture</p>,
              "pull-request": (
                <ScrollArea className="h-full min-h-0 flex-1">
                  <div className="text-metadata p-4">
                    <GitHubPullRequestPanel
                      api={pullRequestPanelApi}
                      branch="feat/sidebar-usage-widget"
                      cwd="/ui-lab/acme/code-two"
                    />
                  </div>
                </ScrollArea>
              ),
            }}
            onClose={() => {
              /* empty */
            }}
            onTab={setTab}
            onWidth={() => {
              /* empty */
            }}
            open
            tab={tab}
            width={420}
          />
        </div>
      </div>
    </ScenarioShell>
  );
}

function Catalog() {
  const { preference } = useTheme();
  const { locale } = useLanguage();
  return (
    <div className="ui-lab-root">
      <aside className="ui-lab-sidebar">
        <div className="ui-lab-brand">
          <span>C2</span>
          <div>
            <strong>UI Lab</strong>
            <small>Development catalog</small>
          </div>
        </div>
        <nav aria-label="UI Lab sections">
          <a aria-current="page" href={uiLabHref("home", preference, locale)}>
            Catalog
          </a>
          <a href="#production-scenarios">Production scenarios</a>
          <a href="#existing-previews">Existing previews</a>
        </nav>
        <div className="ui-lab-sidebar-note">
          <span className="ui-lab-dev-badge">Dev only</span>
          <p>
            Fixtures are local, deterministic, and never call a bridge or remote
            service.
          </p>
        </div>
      </aside>

      <main className="ui-lab-main">
        <header className="ui-lab-header">
          <div>
            <span className="ui-lab-eyebrow">C2 interface reference</span>
            <h1>UI Lab</h1>
            <p>
              Stable production-component scenarios for design review,
              regression checks, and screenshots.
            </p>
          </div>
          <div className="ui-lab-header-controls">
            <ThemeLinks route="home" />
            <LocaleLinks route="home" />
          </div>
        </header>

        <section
          id="production-scenarios"
          className="ui-lab-section"
          aria-labelledby="production-title"
        >
          <div className="ui-lab-section-heading">
            <div>
              <span>01</span>
              <h2 id="production-title">Production scenarios</h2>
            </div>
            <p>These pages render the same components used in C2.</p>
          </div>
          <div className="ui-lab-card-grid">
            {catalogCards.map((card) => (
              <CatalogCard key={card.route} card={card} />
            ))}
          </div>
        </section>

        <section
          id="existing-previews"
          className="ui-lab-section"
          aria-labelledby="previews-title"
        >
          <div className="ui-lab-section-heading">
            <div>
              <span>02</span>
              <h2 id="previews-title">Focused previews</h2>
            </div>
            <p>
              Existing single-purpose development surfaces remain directly
              accessible.
            </p>
          </div>
          <div className="ui-lab-preview-grid">
            {previewCards.map((card) => (
              <PreviewCard key={card.href} card={card} />
            ))}
          </div>
        </section>
      </main>
    </div>
  );
}

export function UiLab({ route: routeOverride }: { route?: UiLabRoute }) {
  const route = routeOverride ?? routeFromLocation();
  const { preference } = useTheme();
  const { locale } = useLanguage();
  if (route === "design-system") {
    return (
      <DesignSystemPreview
        catalogHref={uiLabHref("home", preference, locale)}
        initialThemeMode={preference}
      />
    );
  }
  if (route === "pull-requests") return <PullRequestsScenario />;
  if (route === "pr-dock") return <PullRequestDockScenario />;
  if (route === "settings")
    return (
      <ScenarioShell route="settings" title="Settings controls">
        <SettingsScenario />
      </ScenarioShell>
    );
  return <Catalog />;
}
