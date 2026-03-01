const API_BASE = "http://localhost:5000/api";
const content = document.getElementById("content");

async function init() {
  const { token } = await chrome.storage.local.get("token");

  if (!token) {
    showLogin();
    return;
  }

  try {
    const response = await fetch(`${API_BASE}/files?limit=10&sortBy=date&sortOrder=desc`, {
      headers: { Authorization: `Bearer ${token}` },
    });

    if (response.status === 401) {
      showLogin();
      return;
    }

    const data = await response.json();
    showInbox(data.items || []);
  } catch (err) {
    content.innerHTML = `
      <div class="loading">
        <p style="color: #B4975A">Unable to connect</p>
        <p style="margin-top: 8px; color: #55556C; font-size: 11px">Make sure Center Point is running</p>
      </div>`;
  }
}

function showLogin() {
  content.innerHTML = `
    <div class="login-prompt">
      <div class="logo" style="width:48px;height:48px;font-size:20px;margin-bottom:16px">CP</div>
      <h2>Sign In</h2>
      <p>Open Center Point Inbox to connect your accounts</p>
      <button class="btn btn-primary" style="width:100%;max-width:200px" onclick="openApp()">Open Center Point</button>
    </div>`;
}

function showInbox(items) {
  if (items.length === 0) {
    content.innerHTML = `
      <div class="loading">
        <p>Your inbox is empty</p>
        <p style="margin-top: 4px; color: #55556C; font-size: 11px">Connect an email account to get started</p>
      </div>`;
    return;
  }

  const html = `
    <div class="search-bar">
      <input type="text" placeholder="Search emails and files..." id="searchInput">
    </div>
    <div class="inbox-list">
      ${items.map((item) => `
        <div class="email-item ${!item.isRead ? 'unread' : ''}" data-id="${item.id}">
          <div class="email-avatar">${(item.from || item.title || "?")[0].toUpperCase()}</div>
          <div class="email-content">
            <div class="email-from">${item.from || item.title}</div>
            <div class="email-subject">${item.title}</div>
            <div class="email-snippet">${item.snippet || ""}</div>
          </div>
          <div class="email-meta">
            <div class="email-time">${formatTime(item.updatedAt || item.receivedAt)}</div>
            ${item.hasAttachments ? '<div class="badge badge-gold">Att</div>' : ""}
          </div>
        </div>
      `).join("")}
    </div>`;
  content.innerHTML = html;

  document.querySelectorAll(".email-item").forEach((el) => {
    el.addEventListener("click", () => {
      chrome.tabs.create({ url: `http://localhost:3000/file/${el.dataset.id}` });
    });
  });
}

function formatTime(dateStr) {
  if (!dateStr) return "";
  const date = new Date(dateStr);
  const now = new Date();
  const diffMin = Math.floor((now - date) / 60000);
  if (diffMin < 1) return "now";
  if (diffMin < 60) return `${diffMin}m`;
  if (diffMin < 1440) return `${Math.floor(diffMin / 60)}h`;
  return date.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

function openApp() {
  chrome.runtime.sendMessage({ type: "OPEN_APP" });
}

document.getElementById("openAppBtn").addEventListener("click", openApp);
document.getElementById("composeBtn").addEventListener("click", () => {
  chrome.tabs.create({ url: "http://localhost:3000/dashboard" });
});

init();
