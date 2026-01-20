import {
  SlashCommandBuilder,
  EmbedBuilder,
  type ChatInputCommandInteraction,
} from "discord.js";
import { apiClient } from "../api-client";

export const status = {
  data: new SlashCommandBuilder()
    .setName("status")
    .setDescription("Check the status of an AI agent")
    .addStringOption((option) =>
      option
        .setName("task_id")
        .setDescription("The task ID to check")
        .setRequired(true)
    ),

  async execute(interaction: ChatInputCommandInteraction) {
    const taskId = interaction.options.getString("task_id", true);

    await interaction.deferReply();

    try {
      const task = await apiClient.getTask(taskId);

      const embed = new EmbedBuilder()
        .setColor(getStatusColor(task.status))
        .setTitle("Task Status")
        .addFields(
          { name: "Task ID", value: `\`${task.id}\``, inline: true },
          { name: "Status", value: formatStatus(task.status), inline: true },
          {
            name: "Created",
            value: `<t:${Math.floor(new Date(task.created_at).getTime() / 1000)}:R>`,
            inline: true,
          },
          { name: "Prompt", value: truncate(task.prompt, 1024) }
        )
        .setTimestamp();

      if (task.started_at) {
        embed.addFields({
          name: "Started",
          value: `<t:${Math.floor(new Date(task.started_at).getTime() / 1000)}:R>`,
          inline: true,
        });
      }

      if (task.completed_at) {
        embed.addFields({
          name: "Completed",
          value: `<t:${Math.floor(new Date(task.completed_at).getTime() / 1000)}:R>`,
          inline: true,
        });
      }

      if (task.exit_code !== null && task.exit_code !== undefined) {
        embed.addFields({
          name: "Exit Code",
          value: `\`${task.exit_code}\``,
          inline: true,
        });
      }

      if (task.error_message) {
        embed.addFields({
          name: "Error",
          value: truncate(task.error_message, 1024),
        });
      }

      // Add SSH info if available
      if (task.ssh_command && task.status === "running") {
        embed.addFields({
          name: "SSH Access",
          value: `\`${task.ssh_command}\``,
          inline: false,
        });
      }

      let content = "";
      if (task.status !== "terminated") {
        content = `**Open in browser:** ${task.web_url}`;
        if (task.ip_address && task.status === "running") {
          content += `\n**SSH:** \`ssh root@${task.ip_address}\``;
        }
      }

      await interaction.editReply({
        content,
        embeds: [embed],
      });
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "Unknown error occurred";
      await interaction.editReply({
        content: `Failed to get status: ${message}`,
      });
    }
  },
};

function formatStatus(status: string): string {
  const statusEmojis: Record<string, string> = {
    pending: "⏳ Pending",
    starting: "🚀 Starting",
    running: "▶️ Running",
    suspended: "⏸️ Suspended",
    terminated: "⏹️ Terminated",
  };
  return statusEmojis[status] || status;
}

function getStatusColor(status: string): number {
  const colors: Record<string, number> = {
    pending: 0xffa500, // Orange
    starting: 0x00bfff, // Blue
    running: 0x00ff00, // Green
    suspended: 0xffff00, // Yellow
    terminated: 0x808080, // Gray
  };
  return colors[status] || 0x5865f2;
}

function truncate(str: string, maxLength: number): string {
  if (str.length <= maxLength) return str;
  return str.slice(0, maxLength - 3) + "...";
}
