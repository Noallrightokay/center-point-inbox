const API_BASE = "http://localhost:5000/api";

// Check for new emails periodically
chrome.alarms.create("checkInbox", { periodInMinutes: 2 });

chrome.alarms.onAlarm.addListener(async (alarm) => {
  if (alarm.name === "checkInbox") {
    try {
      const { token } = await chrome.storage.local.get("token");
      if (!token) return;

      const response = await fetch(`${API_BASE}/files?limit=5&sortBy=date&sortOrder=desc`, {
        headers: { Authorization: `Bearer ${token}` },
      });

      if (response.ok) {
        const data = await response.json();
        const unread = data.items?.filter((f) => !f.isRead) || [];

        if (unread.length > 0) {
          chrome.action.setBadgeText({ text: String(unread.length) });
          chrome.action.setBadgeBackgroundColor({ color: "#B4975A" });
        } else {
          chrome.action.setBadgeText({ text: "" });
        }
      }
    } catch (err) {
      console.error("Center Point: inbox check failed", err);
    }
  }
});

// Handle messages from popup/sidebar
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === "AUTH_TOKEN") {
    chrome.storage.local.set({ token: message.token });
    sendResponse({ success: true });
  }
  if (message.type === "OPEN_APP") {
    chrome.tabs.create({ url: "http://localhost:3000/dashboard" });
    sendResponse({ success: true });
  }
  return true;
});
