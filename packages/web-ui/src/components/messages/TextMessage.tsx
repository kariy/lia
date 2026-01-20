import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneLight } from "react-syntax-highlighter/dist/esm/styles/prism";
import type { ParsedTextMessage } from "../../types/claude-stream";
import { cn } from "@/lib/utils";

interface TextMessageProps {
  message: ParsedTextMessage;
}

export function TextMessage({ message }: TextMessageProps) {
  return (
    <div className="flex gap-3">
      <div className="flex-shrink-0 w-6 h-6 rounded-full bg-foreground/10 flex items-center justify-center">
        <span className="text-xs font-medium text-foreground/70">C</span>
      </div>
      <div className={cn("flex-1 min-w-0", message.isStreaming && "animate-pulse")}>
        <div className="prose prose-sm max-w-none text-foreground prose-headings:text-foreground prose-p:text-foreground prose-strong:text-foreground prose-code:text-foreground">
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            components={{
              code({ className, children, ...props }) {
                const match = /language-(\w+)/.exec(className || "");
                const codeString = String(children).replace(/\n$/, "");

                // Check if this is an inline code or a code block
                const isInline = !match && !codeString.includes("\n");

                if (isInline) {
                  return (
                    <code className="bg-secondary px-1.5 py-0.5 rounded text-xs font-mono" {...props}>
                      {children}
                    </code>
                  );
                }

                return (
                  <SyntaxHighlighter
                    style={oneLight}
                    language={match ? match[1] : "text"}
                    PreTag="div"
                    customStyle={{
                      margin: 0,
                      borderRadius: "0.375rem",
                      fontSize: "0.75rem",
                    }}
                  >
                    {codeString}
                  </SyntaxHighlighter>
                );
              },
            }}
          >
            {message.content || " "}
          </ReactMarkdown>
        </div>
        {message.isStreaming && (
          <span className="inline-block w-2 h-4 bg-foreground/50 animate-pulse ml-1" />
        )}
      </div>
    </div>
  );
}
