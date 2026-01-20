import { useEffect } from "react";
import { MessageList } from "../components/messages";
import { TaskHeader } from "../components/TaskHeader";
import { InputBar } from "../components/InputBar";
import { useTaskStore } from "../store";
import type { TaskResponse } from "@lia/shared";
import { Button } from "@/components/ui/button";
import { RotateCcw } from "lucide-react";

// Mock task for the header
const mockTask: TaskResponse = {
  id: "demo-task-1234-5678-90ab-cdef",
  user_id: "demo-user",
  guild_id: null,
  status: "running",
  source: "web",
  repositories: ["facebook/react"],
  vm_id: "vm-demo",
  config: null,
  created_at: new Date().toISOString(),
  started_at: new Date().toISOString(),
  completed_at: null,
  exit_code: null,
  error_message: null,
  web_url: "http://localhost:5174/demo",
  ssh_command: "ssh user@172.16.0.100",
  ip_address: "172.16.0.100",
};

// Simulate streaming messages
const mockStreamMessages = [
  // System init
  '{"type":"system","subtype":"init","session_id":"demo-session","model":"claude-sonnet-4-20250514","tools":["Bash","Read","Write","Edit","Glob","Grep"],"cwd":"/workspace"}',

  // Assistant text message
  '{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I\'ll help you set up a new React project. Let me first check the current directory structure."}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"demo"}',

  // Tool call - Bash ls
  '{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_bash_1","name":"Bash","input":{}}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\\"command\\":\\"ls -la\\",\\"description\\":\\"List files in current directory\\"}"}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"demo"}',

  // Tool result
  '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_bash_1","content":"total 24\\ndrwxr-xr-x  4 user user 4096 Jan 20 10:00 .\\ndrwxr-xr-x 10 user user 4096 Jan 20 09:00 ..\\n-rw-r--r--  1 user user  220 Jan 20 10:00 package.json\\n-rw-r--r--  1 user user  150 Jan 20 10:00 tsconfig.json\\ndrwxr-xr-x  2 user user 4096 Jan 20 10:00 src","is_error":false}]},"session_id":"demo"}',

  // More assistant text
  '{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I can see the project structure. Now let me read the `package.json` to understand the dependencies."}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"demo"}',

  // Tool call - Read file
  '{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_read_1","name":"Read","input":{}}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\\"file_path\\":\\"/workspace/package.json\\"}"}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"demo"}',

  // Tool result
  '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_read_1","content":"     1\\t{\\n     2\\t  \\"name\\": \\"my-app\\",\\n     3\\t  \\"version\\": \\"1.0.0\\",\\n     4\\t  \\"dependencies\\": {\\n     5\\t    \\"react\\": \\"^18.2.0\\",\\n     6\\t    \\"react-dom\\": \\"^18.2.0\\"\\n     7\\t  }\\n     8\\t}","is_error":false}]},"session_id":"demo"}',

  // Assistant explanation with markdown
  '{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"The project has **React 18** installed. Here\'s what I found:\\n\\n- Basic React setup with `react` and `react-dom`\\n- No build tools configured yet\\n\\nLet me create a simple component for you:\\n\\n```tsx\\nexport function App() {\\n  return <h1>Hello World</h1>;\\n}\\n```\\n\\nI\'ll now write this to a file."}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"demo"}',

  // Tool call - Write file
  '{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_write_1","name":"Write","input":{}}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\\"file_path\\":\\"/workspace/src/App.tsx\\",\\"content\\":\\"export function App() {\\\\n  return <h1>Hello World</h1>;\\\\n}\\\\n\\"}"}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"demo"}',

  // Tool result
  '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_write_1","content":"File written successfully to /workspace/src/App.tsx","is_error":false}]},"session_id":"demo"}',

  // Tool call - Edit file
  '{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_edit_1","name":"Edit","input":{}}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\\"file_path\\":\\"/workspace/src/App.tsx\\",\\"old_string\\":\\"Hello World\\",\\"new_string\\":\\"Hello, React!\\"}"}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"demo"}',

  // Tool result
  '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_edit_1","content":"File edited successfully","is_error":false}]},"session_id":"demo"}',

  // Tool call - Grep
  '{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_grep_1","name":"Grep","input":{}}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\\"pattern\\":\\"function\\",\\"path\\":\\"/workspace/src\\"}"}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"demo"}',

  // Tool result
  '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_grep_1","content":"/workspace/src/App.tsx:1:export function App() {","is_error":false}]},"session_id":"demo"}',

  // Tool call with error
  '{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_bash_err","name":"Bash","input":{}}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\\"command\\":\\"cat /nonexistent/file.txt\\",\\"description\\":\\"Try to read a file that doesn\'t exist\\"}"}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"demo"}',

  // Error tool result
  '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_bash_err","content":"cat: /nonexistent/file.txt: No such file or directory","is_error":true}]},"session_id":"demo"}',

  // Final message
  '{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I\'ve set up the basic React component. The file that didn\'t exist was expected - I was just demonstrating error handling. Your project is ready to go!"}},"session_id":"demo"}',
  '{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"demo"}',

  // Result
  '{"type":"result","subtype":"success","is_error":false,"duration_ms":12500,"num_turns":8,"result":"Project setup complete","session_id":"demo","total_cost_usd":0.0234,"usage":{"input_tokens":1500,"output_tokens":450}}',
];

export function DemoPage() {
  const { setTask, processOutput, reset } = useTaskStore();

  useEffect(() => {
    reset();
    setTask(mockTask);

    // Simulate streaming messages with delays
    let index = 0;
    const interval = setInterval(() => {
      if (index < mockStreamMessages.length) {
        processOutput(mockStreamMessages[index]);
        index++;
      } else {
        clearInterval(interval);
      }
    }, 300); // 300ms between each message

    return () => {
      clearInterval(interval);
      reset();
    };
  }, []);

  const handleReplay = () => {
    reset();
    setTask(mockTask);

    let index = 0;
    const interval = setInterval(() => {
      if (index < mockStreamMessages.length) {
        processOutput(mockStreamMessages[index]);
        index++;
      } else {
        clearInterval(interval);
      }
    }, 300);
  };

  return (
    <div className="flex h-full flex-col bg-background">
      {/* Task header */}
      <TaskHeader task={mockTask} />

      {/* Demo controls */}
      <div className="flex items-center justify-between border-b bg-secondary/30 px-4 py-2">
        <span className="text-sm text-muted-foreground">
          Demo mode - visualizing Claude message components
        </span>
        <Button onClick={handleReplay} variant="outline" size="sm">
          <RotateCcw className="h-4 w-4 mr-2" />
          Replay Demo
        </Button>
      </div>

      {/* Messages */}
      <div className="flex-1 min-h-0 overflow-hidden">
        <MessageList />
      </div>

      {/* Input bar */}
      <InputBar />
    </div>
  );
}
