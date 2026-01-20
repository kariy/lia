import { z } from "zod";

const configSchema = z.object({
  discordToken: z.string().min(1),
  discordClientId: z.string().min(1),
  vmApiUrl: z.string().url().default("http://localhost:3000"),
  webUrl: z.string().url().default("http://localhost:5173"),
});

function loadConfig() {
  const result = configSchema.safeParse({
    discordToken: process.env.DISCORD_TOKEN,
    discordClientId: process.env.DISCORD_CLIENT_ID,
    vmApiUrl: process.env.VM_API_URL,
    webUrl: process.env.WEB_URL,
  });

  if (!result.success) {
    console.error("Configuration error:", result.error.format());
    process.exit(1);
  }

	console.log(result)

  return result.data;
}

export const config = loadConfig();
