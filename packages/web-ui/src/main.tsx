import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter, Routes, Route } from "react-router-dom";
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
    </BrowserRouter>
  </React.StrictMode>
);
