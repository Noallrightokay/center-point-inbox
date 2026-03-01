import axios, {
  AxiosError,
  AxiosInstance,
  AxiosRequestConfig,
  AxiosResponse,
  InternalAxiosRequestConfig,
} from "axios";
import { getToken, refreshToken, clearToken } from "./auth";

/**
 * Configured Axios instance for Center Point Inbox API communication.
 *
 * - Attaches JWT bearer token from in-memory store on every request.
 * - On 401 responses, attempts a silent token refresh and retries the
 *   original request exactly once before rejecting.
 */

const API_BASE_URL =
  process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

const api: AxiosInstance = axios.create({
  baseURL: `${API_BASE_URL}/api`,
  timeout: 30_000,
  headers: {
    "Content-Type": "application/json",
    Accept: "application/json",
  },
  withCredentials: true,
});

// ---------------------------------------------------------------------------
// Request interceptor – attach JWT
// ---------------------------------------------------------------------------
api.interceptors.request.use(
  (config: InternalAxiosRequestConfig) => {
    const token = getToken();
    if (token && config.headers) {
      config.headers.Authorization = `Bearer ${token}`;
    }
    return config;
  },
  (error: AxiosError) => Promise.reject(error),
);

// ---------------------------------------------------------------------------
// Response interceptor – silent refresh on 401
// ---------------------------------------------------------------------------

/** Prevents parallel refresh attempts. */
let isRefreshing = false;

/** Queue of requests waiting for the token refresh to finish. */
let failedQueue: Array<{
  resolve: (token: string | null) => void;
  reject: (error: unknown) => void;
}> = [];

function processQueue(error: unknown, token: string | null = null): void {
  failedQueue.forEach((prom) => {
    if (error) {
      prom.reject(error);
    } else {
      prom.resolve(token);
    }
  });
  failedQueue = [];
}

api.interceptors.response.use(
  (response: AxiosResponse) => response,
  async (error: AxiosError) => {
    const originalRequest = error.config as InternalAxiosRequestConfig & {
      _retry?: boolean;
    };

    // Only attempt refresh for 401 responses that haven't already been retried.
    if (error.response?.status !== 401 || originalRequest._retry) {
      return Promise.reject(error);
    }

    if (isRefreshing) {
      // Another request is already refreshing – queue this one.
      return new Promise<string | null>((resolve, reject) => {
        failedQueue.push({ resolve, reject });
      }).then((token) => {
        if (token && originalRequest.headers) {
          originalRequest.headers.Authorization = `Bearer ${token}`;
        }
        return api(originalRequest);
      });
    }

    originalRequest._retry = true;
    isRefreshing = true;

    try {
      const newToken = await refreshToken();
      processQueue(null, newToken);
      if (newToken && originalRequest.headers) {
        originalRequest.headers.Authorization = `Bearer ${newToken}`;
      }
      return api(originalRequest);
    } catch (refreshError) {
      processQueue(refreshError, null);
      clearToken();
      // Redirect to login when the refresh fails.
      if (typeof window !== "undefined") {
        window.location.href = "/login";
      }
      return Promise.reject(refreshError);
    } finally {
      isRefreshing = false;
    }
  },
);

// ---------------------------------------------------------------------------
// Typed request helpers
// ---------------------------------------------------------------------------

/**
 * Typed GET request.
 *
 * @example
 * const users = await get<User[]>("/users");
 */
export async function get<T>(
  url: string,
  config?: AxiosRequestConfig,
): Promise<T> {
  const response = await api.get<T>(url, config);
  return response.data;
}

/**
 * Typed POST request.
 *
 * @example
 * const created = await post<User>("/users", { name: "Alice" });
 */
export async function post<T>(
  url: string,
  data?: unknown,
  config?: AxiosRequestConfig,
): Promise<T> {
  const response = await api.post<T>(url, data, config);
  return response.data;
}

/**
 * Typed PUT request.
 *
 * @example
 * const updated = await put<User>("/users/1", { name: "Bob" });
 */
export async function put<T>(
  url: string,
  data?: unknown,
  config?: AxiosRequestConfig,
): Promise<T> {
  const response = await api.put<T>(url, data, config);
  return response.data;
}

/**
 * Typed PATCH request.
 *
 * @example
 * const patched = await patch<User>("/users/1", { name: "Carol" });
 */
export async function patch<T>(
  url: string,
  data?: unknown,
  config?: AxiosRequestConfig,
): Promise<T> {
  const response = await api.patch<T>(url, data, config);
  return response.data;
}

/**
 * Typed DELETE request.
 *
 * @example
 * await del<void>("/users/1");
 */
export async function del<T>(
  url: string,
  config?: AxiosRequestConfig,
): Promise<T> {
  const response = await api.delete<T>(url, config);
  return response.data;
}

export default api;
