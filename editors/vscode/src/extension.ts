import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Trace
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let outputChannel: vscode.OutputChannel;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  outputChannel = vscode.window.createOutputChannel("PDL Language Server");
  context.subscriptions.push(outputChannel);

  context.subscriptions.push(
    vscode.commands.registerCommand("pdl.restartServer", async () => {
      await restartClient(context);
    }),
    vscode.commands.registerCommand("pdl.showOutput", () => {
      outputChannel.show();
    }),
    vscode.workspace.onDidChangeConfiguration(async (event) => {
      if (
        event.affectsConfiguration("pdl.server.path") ||
        event.affectsConfiguration("pdl.server.args")
      ) {
        await restartClient(context);
      } else if (event.affectsConfiguration("pdl.trace.server") && client) {
        await client.setTrace(traceFromConfiguration());
      }
    })
  );

  await restartClient(context);
}

export async function deactivate(): Promise<void> {
  if (!client) {
    return;
  }
  const activeClient = client;
  client = undefined;
  await activeClient.stop();
}

async function restartClient(context: vscode.ExtensionContext): Promise<void> {
  if (client) {
    const activeClient = client;
    client = undefined;
    await activeClient.stop();
  }

  const configuration = vscode.workspace.getConfiguration("pdl");
  const command = configuration.get<string>("server.path", "pdl");
  const args = configuration.get<string[]>("server.args", ["lsp"]);
  const options = {
    cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath
  };
  const serverOptions: ServerOptions = {
    run: { command, args, options },
    debug: { command, args, options }
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { language: "pdl", scheme: "file" },
      { language: "pdl", scheme: "untitled" }
    ],
    synchronize: {
      configurationSection: "pdl"
    },
    outputChannel,
    traceOutputChannel: outputChannel
  };

  client = new LanguageClient("pdl", "PDL Language Server", serverOptions, clientOptions);
  context.subscriptions.push(client);
  await client.start();
  await client.setTrace(traceFromConfiguration());
}

function traceFromConfiguration(): Trace {
  const value = vscode.workspace
    .getConfiguration("pdl")
    .get<string>("trace.server", "off");
  switch (value) {
    case "messages":
      return Trace.Messages;
    case "verbose":
      return Trace.Verbose;
    default:
      return Trace.Off;
  }
}
