import {
  SlashCommandBuilder,
  EmbedBuilder,
  type ChatInputCommandInteraction,
} from "discord.js";
import { apiClient } from "../api-client";

export const list = {
  data: new SlashCommandBuilder()
    .setName("list")
    .setDescription("List your active and suspended AI agents")
    .addStringOption((option) =>
      option
        .setName("status")
        .setDescription("Filter by status")
        .setRequired(false)
        .addChoices(
          { name: "All", value: "all" },
          { name: "Running", value: "running" },
          { name: "Suspended", value: "suspended" },
          { name: "Pending", value: "pending" }
        )
    ),

  async execute(interaction: ChatInputCommandInteraction) {
    const statusFilter = interaction.options.getString("status") || "all";

    await interaction.deferReply();

    try {
      const result = await apiClient.listTasks(
        interaction.user.id,
        statusFilter !== "all" ? statusFilter : undefined
      );

      if (result.tasks.length === 0) {
        await interaction.editReply({
          content: "You have no active AI agents.",
        });
        return;
      }

      const embed = new EmbedBuilder()
        .setColor(0x5865f2)
        .setTitle("Your AI Agents")
        .setDescription(`Showing ${result.tasks.length} of ${result.total} agents`)
        .setTimestamp();

      for (const task of result.tasks.slice(0, 10)) {
        const createdAt = Math.floor(
          new Date(task.created_at).getTime() / 1000
        );
        const repoDisplay = task.repositories.length > 0
          ? truncate(task.repositories.join(", "), 50)
          : "No repository";
        embed.addFields({
          name: `${formatStatus(task.status)} ${repoDisplay}`,
          value: `ID: \`${task.id}\`\nCreated: <t:${createdAt}:R>\n[Open](${task.web_url})`,
          inline: false,
        });
      }

      if (result.total > 10) {
        embed.setFooter({
          text: `Showing 10 of ${result.total} agents. Use the web UI to see all.`,
        });
      }

      await interaction.editReply({
        embeds: [embed],
      });
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "Unknown error occurred";
      await interaction.editReply({
        content: `Failed to list agents: ${message}`,
      });
    }
  },
};

function formatStatus(status: string): string {
  const statusEmojis: Record<string, string> = {
    pending: "⏳",
    starting: "🚀",
    running: "▶️",
    suspended: "⏸️",
    terminated: "⏹️",
  };
  return statusEmojis[status] || "❓";
}

function truncate(str: string, maxLength: number): string {
  if (str.length <= maxLength) return str;
  return str.slice(0, maxLength - 3) + "...";
}
