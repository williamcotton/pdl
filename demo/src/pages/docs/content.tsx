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
  | filter status == "completed"
  | group_by region
  | agg total_revenue = sum(amount), orders = count()
  | sort total_revenue desc
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
  | filter status == "completed"
  | select region, customer_id, amount
  | sort amount desc
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
  | filter status == "completed"
  | select order_id, region, amount
  | sort order_id
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
  | filter lower(trim(status)) == "completed"
  | mutate
      net_amount = gross_amount - coalesce(discount, 0),
      region_channel = concat(upper(trim(region)), ":", lower(trim(channel))),
      priority = if_else(gross_amount >= 150, "high", "standard")
  | distinct order_id
  | select order_id, region_channel, net_amount, priority
  | sort order_id
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
  | select customer_id, segment

load "sales.csv"
  | filter status == "completed"
  | join customers on customer_id kind left
  | group_by segment
  | agg revenue = sum(amount), orders = count()
  | sort revenue desc
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
  | sort order_id
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
        Window expressions add analytics without collapsing the table. Use them when each input row should
        stay visible, but each row needs context from related rows, such as customer totals, regional ranks,
        running sums, previous values, or first and last values.
      </>
    ),
    sections: [
      {
        id: "one-window",
        title: "Step 1: rank one table",
        body: (
          <>
            <p>
              Start with the smallest useful window: one calculation over the whole filtered table. This
              pipeline keeps completed sales, then adds <Code>sale_rank</Code> with{" "}
              <Code>row_number()</Code>.
            </p>
            <ol>
              <li>
                <Code>row_number()</Code> is the calculation. It writes <Code>1</Code>, <Code>2</Code>,{" "}
                <Code>3</Code>, and so on.
              </li>
              <li>
                <Code>over (...)</Code> says this is a window expression instead of a plain scalar
                function.
              </li>
              <li>
                <Code>order_by amount desc</Code> tells the window how to rank the rows: largest sale
                first.
              </li>
              <li>
                There is no <Code>partition_by</Code> yet, so all completed sales share one window.
              </li>
            </ol>
            <p>
              Notice the final <Code>sort sale_rank</Code>. The window's <Code>order_by</Code> controls
              the calculation, while a pipeline <Code>sort</Code> controls how the output is displayed.
            </p>
          </>
        ),
        example: {
          id: "windows-one-table-rank",
          files: SALES_FILES,
          source: `load "sales.csv"
  | filter status == "completed"
  | mutate
      sale_rank =
        row_number() over (
          order_by amount desc
        )
  | select region, customer_id, amount, sale_rank
  | sort sale_rank
`,
        },
      },
      {
        id: "partitioned-window",
        title: "Step 2: restart per customer",
        body: (
          <>
            <p>
              Add <Code>partition_by</Code> when each group should get its own independent calculation.
              Here the row number restarts for every <Code>customer_id</Code>.
            </p>
            <ol>
              <li>
                PDL first forms one customer partition for <Code>C001</Code>, one for <Code>C003</Code>,
                and so on.
              </li>
              <li>
                Inside each customer partition, <Code>order_by amount desc</Code> puts that customer's
                largest completed sale first.
              </li>
              <li>
                <Code>row_number()</Code> then writes <Code>1</Code>, <Code>2</Code>, and so on within
                that customer only.
              </li>
            </ol>
            <p>
              Customer <Code>C001</Code> has two completed sales in the input. In the output, only those
              two rows share the same numbering sequence.
            </p>
          </>
        ),
        example: {
          id: "windows-partitioned-rank",
          files: SALES_FILES,
          source: `load "sales.csv"
  | filter status == "completed"
  | mutate
      customer_sale_number =
        row_number() over (
          partition_by customer_id
          order_by amount desc
        )
  | select customer_id, amount, customer_sale_number
  | sort customer_id, customer_sale_number
`,
        },
      },
      {
        id: "partition-totals",
        title: "Step 3: add totals without collapsing rows",
        body: (
          <>
            <p>
              A grouped aggregate answers one row per group. A window aggregate answers the same question,
              then repeats the answer on every row in the partition. That is the main reason windows are
              useful during cleanup and feature engineering.
            </p>
            <ol>
              <li>
                <Code>sum(amount) over (partition_by customer_id)</Code> gives each row that
                customer's total completed revenue.
              </li>
              <li>
                <Code>sum(amount) over (partition_by region)</Code> gives each row that region's total
                completed revenue.
              </li>
              <li>
                There is no <Code>order_by</Code> here because a full-partition total does not care about
                row order.
              </li>
            </ol>
            <p>
              Compare the input and output: every completed sale remains present, but each row now carries
              extra context from the rows around it.
            </p>
          </>
        ),
        example: {
          id: "windows-partition-totals",
          files: SALES_FILES,
          source: `load "sales.csv"
  | filter status == "completed"
  | mutate
      customer_revenue =
        sum(amount) over (
          partition_by customer_id
        ),
      region_revenue =
        sum(amount) over (
          partition_by region
        )
  | select
      region,
      customer_id,
      amount,
      customer_revenue,
      region_revenue
  | sort region, customer_id, amount desc
`,
        },
      },
      {
        id: "rank-derived-total",
        title: "Step 4: use a window result in the next stage",
        body: (
          <>
            <p>
              Assignments inside one <Code>mutate</Code> stage are parallel. If a second window expression
              needs a column you just created, put it in the next <Code>mutate</Code>.
            </p>
            <ol>
              <li>
                The first <Code>mutate</Code> creates <Code>region_revenue</Code> on every completed sale.
              </li>
              <li>
                The second <Code>mutate</Code> ranks rows by that new <Code>region_revenue</Code> value.
              </li>
              <li>
                <Code>dense_rank()</Code> gives tied rows the same rank and does not leave gaps after ties.
              </li>
            </ol>
          </>
        ),
        example: {
          id: "windows-rank-derived-total",
          files: SALES_FILES,
          source: `load "sales.csv"
  | filter status == "completed"
  | mutate
      region_revenue =
        sum(amount) over (
          partition_by region
        )
  | mutate
      region_revenue_rank =
        dense_rank() over (
          order_by region_revenue desc
        )
  | select
      region,
      customer_id,
      amount,
      region_revenue,
      region_revenue_rank
  | sort region_revenue_rank, customer_id, amount desc
`,
        },
      },
      {
        id: "running-frames",
        title: "Step 5: running frames and previous rows",
        body: (
          <>
            <p>
              Add a row frame when the calculation should see only part of the ordered partition. This is
              how you get running totals, moving averages, and other row-by-row accumulations.
            </p>
            <ol>
              <li>
                <Code>partition_by customer_id</Code> keeps each customer's running total separate.
              </li>
              <li>
                <Code>order_by amount desc</Code> gives each customer a stable sequence.
              </li>
              <li>
                <Code>frame running</Code> means "from the first row in
                this ordered customer partition through this row."
              </li>
              <li>
                <Code>lag(amount)</Code> uses the same partition and order, then reads the previous row's
                amount.
              </li>
            </ol>
            <p>
              A blank previous amount means the row is first in its customer partition, so there is no
              earlier row to read.
            </p>
          </>
        ),
        example: {
          id: "windows-running-frames",
          files: SALES_FILES,
          source: `load "sales.csv"
  | filter status == "completed"
  | mutate
      sale_rank =
        row_number() over (
          partition_by customer_id
          order_by amount desc
        ),
      running_customer_revenue =
        sum(amount) over (
          partition_by customer_id
          order_by amount desc
          frame running
        ),
      previous_sale_amount =
        lag(amount) over (
          partition_by customer_id
          order_by amount desc
        )
  | select
      customer_id,
      amount,
      sale_rank,
      running_customer_revenue,
      previous_sale_amount
  | sort customer_id, sale_rank
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
