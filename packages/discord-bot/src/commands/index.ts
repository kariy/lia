import { spawn } from "./spawn";
import { spawnFile } from "./spawn-file";
import { status } from "./status";
import { resume } from "./resume";
import { stop } from "./stop";
import { list } from "./list";

export const commands = [spawn, spawnFile, status, resume, stop, list];
