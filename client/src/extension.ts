"use strict";

import * as vscode from "vscode";
import { execFile } from "mz/child_process";
import * as net from "net";

import { LanguageClient, LanguageClientOptions, StreamInfo } from "vscode-languageclient/node";
import { workspace } from "vscode";

function delay(ms: number) {
  return new Promise( resolve => setTimeout(resolve, ms) );
}

function bindCustomEvents(client: LanguageClient, context: vscode.ExtensionContext, settings) {
  let disposables = []

  client.onNotification("workspace/elasticRubyServerBusy", (params) => {
    console.log("received status:")
    console.log(params)

    if (params.busy == "true") {
      statusBarItem.color = "#781720";
      statusBarItem.tooltip = `${params.tooltip}`;
    } else {
      statusBarItem.color = undefined;
      statusBarItem.tooltip = ``;
    }
  });

	disposables.push(
    vscode.commands.registerCommand("elasticRubyServer.reindexWorkspace", () => {
      vscode.window.withProgress({ title: "Elastic Ruby Client", location: vscode.ProgressLocation.Window }, async progress => {
        progress.report({ message: "Reindexing workspace..." });
        client.sendNotification("workspace/reindex");
      });
    })
  );

	// disposables.push(
  //   vscode.commands.registerCommand("elasticRubyServer.reindexGems", () => {
  //     vscode.window.withProgress({ title: "Elastic Ruby Client", location: vscode.ProgressLocation.Window }, async progress => {
  //       progress.report({ message: "Reindexing gems..." });
  //       client.sendNotification("workspace/reindexGems");
  //     });
  //   })
  // );

	disposables.push(
    vscode.commands.registerCommand("elasticRubyServer.stopServer", () => {
      vscode.window.withProgress({ title: "Elastic Ruby Client", location: vscode.ProgressLocation.Window }, async progress => {
        progress.report({ message: "Stopping server..." });
        execFile("docker", [ "stop", settings.containerName ]);
      });
    })
  );

  for (let disposable of disposables) {
    context.subscriptions.push(disposable);
  }
}

function buildContainerArgs(settings) {
  let dockerArgs = [
    "run",
    "-d",
    "--rm",
    "--name", settings.containerName,
    "--ulimit", "memlock=-1:-1",
    "-v", `${settings.volumeName}:/usr/share/elasticsearch/data`,
    "-p", `${settings.port}:${settings.port}`,
    "-e", `SERVER_PORT=${settings.port}`,
    "-e", `LOG_LEVEL=${settings.logLevel}`,
    "-e", `HOST_PROJECT_ROOTS="${settings.projectPaths.join(",")}"`
    // "-e", `HOST_PROJECT_ROOTS="${settings.projectPaths.join(",")}"`,
    // "-e", `GEMS_PATH="/Users/joelkorpela/.rbenv/versions/"`
  ];

  const mounts = settings.projectPaths.map(path => {
    return {
      path: path,
      name: path.match(/\/([^\/]*?)(\/$|$)/)[1]
    };
  });

  mounts.forEach(mount => {
    dockerArgs.push(
      "--mount",
      // `type=bind,source=${mount.path},target=/projects/${mount.name},readonly`
      `type=bind,source=${mount.path},target=/projects/${mount.name}`
    );
  });

  // dockerArgs.push(
  //   "--mount",
  //   `type=bind,source=/Users/joelkorpela/.rbenv/versions/,target=/gems/,readonly`
  // );

  dockerArgs.push(settings.image);

  return dockerArgs;
}

async function pullImage(image: string) {
  var attempts = 0;
  while (attempts < 10) {
    try {
      await vscode.window.withProgress({ title: "elastic_ruby_server", location: vscode.ProgressLocation.Window }, async progress => {
        progress.report({ message: `Pulling ${image}` });
        await execFile("docker", ["pull", image], {});
        attempts = attempts + 10
      });
    } catch (err) {
      attempts = attempts + 1
      // vscode.window.showErrorMessage(`${err.code}`);
      if (err.code == 1) { // Docker not yet running
        vscode.window.showErrorMessage("Waiting for docker to start");
        await delay(10 * 1000);
      } else {
        if (err.code == "ENOENT") {
          const selected = await vscode.window.showErrorMessage(
            "Docker executable not found. Install Docker.",
            { modal: true },
            "Open settings"
            );
            if (selected === "Open settings") {
              await vscode.commands.executeCommand("workbench.action.openWorkspaceSettings");
            }
        } else {
          vscode.window.showErrorMessage("Error updating docker image! - will try to use existing local one: " + err.message);
          console.error(err);
        }
      }
    }
  }
}

async function createVolume(volumeName: string) {
  await execFile("docker", ["volume", "create", volumeName]);
}

async function startContainer(settings) {
  // todo: cleanup this function
  try {
    // check if the container is already running
    await execFile("docker", [ "container", "top", settings.containerName ]);
  } catch (error) {
    // it's not running, fire it up!
    await execFile("docker", buildContainerArgs(settings));

    await delay(5 * 1000)
  }

  try {
    await execFile("docker", [ "container", "top", settings.containerName ]);
  } catch (error) {
    // Give it a bit more time, probably will be fine
    await delay(5 * 1000);
  }
}

function buildLanguageClient(port) {
  let serverOptions = () => {
    let socket = net.connect({
      port: port,
      host: "localhost"
    });
    let result: StreamInfo = {
      writer: socket,
      reader: socket
    };

    return Promise.resolve(result);
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: ["ruby"],
    synchronize: {
      // fileEvents: workspace.createFileSystemWatcher("**/*.rb")
      fileEvents: null
    }
  };

  return new LanguageClient(
    "ElasticRubyServer",
    "Elastic Ruby Server",
    serverOptions,
    clientOptions
  );
}

let client: LanguageClient;
let statusBarItem: vscode.StatusBarItem;

export async function activate(context: vscode.ExtensionContext, reactivating = false) {
  if (!workspace.workspaceFolders) { return; }

  statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, -100);
  context.subscriptions.push(statusBarItem);

  statusBarItem.text = `$(ruby)`;
  statusBarItem.color = undefined;
  statusBarItem.tooltip = ``
  statusBarItem.show();

  const conf = vscode.workspace.getConfiguration("elasticRubyServer");
  const settings = {
    image: conf["image"] || "blinknlights/elastic_ruby_server:1.0",
    // image:         conf["image"] || "elastic_ruby_server",
    projectPaths:  conf["projectPaths"],
    port:          conf["port"],
    logLevel:      conf["logLevel"],
    volumeName:    `elastic_ruby_server-9.0.0`,
    containerName: "elastic-ruby-server"
  }

  pullImage(settings.image);
  await createVolume(settings.volumeName);
  await startContainer(settings);

  client = buildLanguageClient(settings.port);

  bindCustomEvents(client, context, settings);

  client.start().catch((error)=> client.error(`Start failed`, error, 'force'));
}

export function deactivate() {
	return client.stop();
}
