import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import { Toaster } from "sonner";
import { Layout } from "./components/Layout";
import { WelcomePage } from "./pages/WelcomePage";
import { TaskPage } from "./pages/TaskPage";
import { DemoPage } from "./pages/DemoPage";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <BrowserRouter>
      <Routes>
        {/* All pages with sidebar layout */}
        <Route element={<Layout />}>
          <Route path="/" element={<WelcomePage />} />
          <Route path="/tasks/:taskId" element={<TaskPage />} />
          <Route path="/demo" element={<DemoPage />} />
        </Route>
      </Routes>
      <Toaster
        position="bottom-left"
        duration={3000}
        theme="light"
        expand={true}
        visibleToasts={5}
        gap={8}
        toastOptions={{
          style: {
            zIndex: 9999,
            background: "white",
            border: "1px solid hsl(0 0% 90%)",
            color: "hsl(0 0% 10%)",
          },
          classNames: {
            success: "",
            error: "",
          },
        }}
      />
    </BrowserRouter>
  </React.StrictMode>
);
