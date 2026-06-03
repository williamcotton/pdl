import React from "react";

import {
  CUSTOMERS_CSV,
  DAILY_ORDERS_DAY1_CSV,
  DAILY_ORDERS_DAY2_CSV,
  ORDERS_JSONL,
  ORDERS_RAW_CSV,
  SALES_CSV,
} from "./datasets";

export interface DocExample {
  id: string;
  source: string;
  files: Record<string, string>;
  stdoutFormat?: "csv" | "jsonl";
}

export interface DocSection {
  id: string;
  title: string;
  body: React.ReactNode;
  example?: DocExample;
}

export interface DocTopic {
  slug: string;
  nav: string;
  title: string;
  lede: React.ReactNode;
  sections: DocSection[];
}

const SALES_FILES = { "sales.csv": SALES_CSV };
const CUSTOMER_FILES = { "sales.csv": SALES_CSV, "customers.csv": CUSTOMERS_CSV };
const ORDERS_FILES = { "orders_raw.csv": ORDERS_RAW_CSV };
const DAILY_FILES = {
  "daily_orders_2026_02_01.csv": DAILY_ORDERS_DAY1_CSV,
  "daily_orders_2026_02_02.csv": DAILY_ORDERS_DAY2_CSV,
};
const JSONL_FILES = { "orders.jsonl": ORDERS_JSONL };

function Code({ children }: { children: React.ReactNode }): React.ReactElement {
  return <code className="doc-inline-code">{children}</code>;
}

export const DOC_TOPICS: DocTopic[] = [
  {
    slug: "",
    nav: "Overview",
    title: "PDL documentation",
    lede: (
      <>
        PDL is a Unix-pipeline-style language for deterministic table preparation. It loads data from files
        or stdin, applies ordered table stages, and writes a table or stream that another tool can consume.
        The live examples on these pages run through the same Rust parser, analyzer, driver, executor, and
        editor-service ABI used by the CLI and LSP.
      </>
    ),
    sections: [
      {
        id: "model",
        title: "The mental model",
        body: (
          <>
            <p>
              A PDL program is a pipeline. <Code>load</Code> creates the first table, each stage after{" "}
              <Code>|</Code> transforms it, and the final table can be streamed to stdout or saved. Stages
              are deterministic, left-to-right, and designed to keep stdout clean when data is being piped.
            </p>
            <p>
              PDL prepares tables for downstream jobs. The intended handoff is ordinary data, especially
              Arrow IPC streams in the native CLI when a consumer needs typed batches.
            </p>
          </>
        ),
        example: {
          id: "overview-top-regions",
          files: SALES_FILES,
          source: `load "sales.csv"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", count() as "orders"
  | sort "total_revenue" desc
`,
        },
      },
      {
        id: "where-next",
        title: "Where to go next",
        body: (
          <p>
            Start with loading and filtering, then move to mutation, grouping, joins, unions, and window
            analytics. The tooling page covers <Code>check</Code>, <Code>fmt</Code>, <Code>schema</Code>,{" "}
            <Code>plan</Code>, <Code>manifest</Code>, <Code>lsp</Code>, and browser WASM behavior.
          </p>
        ),
      },
    ],
  },
  {
    slug: "loading",
    nav: "Loading data",
    title: "Loading data",
    lede: (
      <>
        PDL reads files by path and chooses a format from explicit stage syntax, CLI overrides, extension,
        magic bytes, text sniffing, then CSV fallback. The browser host supplies in-memory files; native CLI
        runs use the filesystem and stdin.
      </>
    ),
    sections: [
      {
        id: "csv",
        title: "CSV input and stdout",
        body: (
          <p>
            CSV is the simplest browser-friendly format. The same pipeline can be checked for schema
            mistakes while editing and then run to produce CSV stdout.
          </p>
        ),
        example: {
          id: "loading-csv",
          files: SALES_FILES,
          source: `load "sales.csv"
  | filter "status" == "completed"
  | select "region", "customer_id", "amount"
  | sort "amount" desc
`,
        },
      },
      {
        id: "json-lines",
        title: "JSON Lines input",
        body: (
          <p>
            JSON Lines is also available in the browser for text input and text stdout. Native execution adds
            Parquet and Arrow IPC file or stream formats where binary output is allowed.
          </p>
        ),
        example: {
          id: "loading-jsonl",
          files: JSONL_FILES,
          source: `load "orders.jsonl"
  | filter "status" == "completed"
  | select "order_id", "region", "amount"
  | sort "order_id"
`,
          stdoutFormat: "jsonl",
        },
      },
    ],
  },
  {
    slug: "transforming",
    nav: "Transforming",
    title: "Filtering, projection, and mutation",
    lede: (
      <>
        Row-preserving stages clean and reshape data before aggregation. String cleanup, null handling, and
        arithmetic live in expressions while stages keep table operations explicit.
      </>
    ),
    sections: [
      {
        id: "cleaning",
        title: "Clean raw orders",
        body: (
          <p>
            <Code>filter</Code> keeps completed orders, <Code>mutate</Code> derives new columns,{" "}
            <Code>distinct</Code> removes duplicate order IDs, and <Code>select</Code> controls the output
            schema.
          </p>
        ),
        example: {
          id: "transform-clean-orders",
          files: ORDERS_FILES,
          source: `load "orders_raw.csv"
  | filter lower(trim("status")) == "completed"
  | mutate
      "net_amount" = "gross_amount" - coalesce("discount", 0),
      "region_channel" = concat(upper(trim("region")), lit(":"), lower(trim("channel"))),
      "priority" = if_else("gross_amount" >= 150, lit("high"), lit("standard"))
  | distinct "order_id"
  | select "order_id", "region_channel", "net_amount", "priority"
  | sort "order_id"
`,
        },
      },
    ],
  },
  {
    slug: "grouping",
    nav: "Grouping & joins",
    title: "Grouping and joins",
    lede: (
      <>
        Named bindings let one pipeline feed another. Use them for reusable cleanup, joins, unions, and
        keeping multi-input programs readable.
      </>
    ),
    sections: [
      {
        id: "join",
        title: "Join customer segments",
        body: (
          <p>
            <Code>let customers = ...</Code> creates a reusable table. The main pipeline joins it by key,
            groups by segment, aggregates revenue, and sorts the result.
          </p>
        ),
        example: {
          id: "grouping-segments",
          files: CUSTOMER_FILES,
          source: `let customers =
  load "customers.csv"
  | select "customer_id", "segment"

load "sales.csv"
  | filter "status" == "completed"
  | join customers on "customer_id" kind left
  | group_by "segment"
  | agg sum("amount") as "revenue", count() as "orders"
  | sort "revenue" desc
`,
        },
      },
      {
        id: "union",
        title: "Union daily files by name",
        body: (
          <p>
            <Code>union day2 by_name true distinct true</Code> lines up columns by name, removes duplicate
            rows, and keeps ordering deterministic with an explicit <Code>sort</Code>.
          </p>
        ),
        example: {
          id: "grouping-union",
          files: DAILY_FILES,
          source: `let day2 =
  load "daily_orders_2026_02_02.csv"

load "daily_orders_2026_02_01.csv"
  | union day2 by_name true distinct true
  | sort "order_id"
`,
        },
      },
    ],
  },
  {
    slug: "windows",
    nav: "Windows",
    title: "Window analytics",
    lede: (
      <>
        Window expressions are row-preserving analytics inside <Code>mutate</Code>. They compute over
        partitions, ordering, and optional row frames without collapsing the table.
      </>
    ),
    sections: [
      {
        id: "customer-metrics",
        title: "Customer and region metrics",
        body: (
          <p>
            Windowed <Code>row_number</Code>, <Code>sum</Code>, and <Code>dense_rank</Code> add analytic
            columns while retaining each completed sale.
          </p>
        ),
        example: {
          id: "windows-customer-metrics",
          files: SALES_FILES,
          source: `load "sales.csv"
  | filter "status" == "completed"
  | mutate
      "customer_sale_number" =
        row_number() over (
          partition_by "customer_id"
          order_by "amount" desc
        ),
      "customer_revenue" =
        sum("amount") over (
          partition_by "customer_id"
        ),
      "region_revenue" =
        sum("amount") over (
          partition_by "region"
        )
  | mutate
      "region_revenue_rank" =
        dense_rank() over (
          order_by "region_revenue" desc
        )
  | select
      "region",
      "customer_id",
      "amount",
      "customer_sale_number",
      "customer_revenue",
      "region_revenue_rank"
  | sort "region_revenue_rank", "customer_id", "amount" desc
`,
        },
      },
    ],
  },
  {
    slug: "tooling",
    nav: "Tooling",
    title: "Tooling and interop",
    lede: (
      <>
        The CLI, LSP, VS Code client, and browser runtime share one implementation. The browser demo is an
        in-memory host around the Rust WASM ABI; it is not a TypeScript PDL implementation.
      </>
    ),
    sections: [
      {
        id: "cli",
        title: "Command line",
        body: (
          <>
            <p>
              Build the binary, run examples, check sources, format files, and inspect schemas or plans from
              the repository root.
            </p>
            <pre className="doc-codeblock">
              <code>{`cargo build -p pdl-cli
cargo run -p pdl-cli -- run examples/top_regions.pdl --stdout-format csv
cargo run -p pdl-cli -- check examples/top_regions.pdl
cargo run -p pdl-cli -- schema examples/top_regions.pdl
cargo run -p pdl-cli -- plan examples/top_regions.pdl --stdout-format csv`}</code>
            </pre>
          </>
        ),
      },
      {
        id: "streams",
        title: "Arrow stream handoff",
        body: (
          <>
            <p>
              Native PDL can stream Arrow IPC to stdout for tools that understand Arrow streams. Keep human
              logs and diagnostics on stderr so stdout remains a clean data stream.
            </p>
            <pre className="doc-codeblock">
              <code>{`pdl run prep.pdl --stdout-format arrow-stream > prepared.arrow
pdl run passthrough.pdl --stdin-format arrow-stream < prepared.arrow > sorted.arrow`}</code>
            </pre>
          </>
        ),
      },
      {
        id: "browser",
        title: "Browser runtime",
        body: (
          <>
            <p>
              In the browser, host files are supplied in memory and stdout previews are text-only. CSV and
              JSON Lines are supported here; Arrow IPC and Parquet browser output controls are deferred.
            </p>
            <pre className="doc-codeblock">
              <code>{`cd demo
npm install
npm run dev`}</code>
            </pre>
          </>
        ),
      },
    ],
  },
];

export function topicForSlug(slug: string): DocTopic {
  return DOC_TOPICS.find((topic) => topic.slug === slug) ?? DOC_TOPICS[0];
}
