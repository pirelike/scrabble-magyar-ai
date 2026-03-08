import time
from collections import defaultdict


class RateLimiter:
    """Generikus rate limiter Socket.IO (SID) és HTTP (IP) kérésekhez."""

    def __init__(self, socket_limits, ip_limits):
        """
        socket_limits: {event_name: (max_requests, window_seconds)}
        ip_limits: {action_name: (max_requests, window_seconds)}
        """
        self._socket_limits = socket_limits
        self._ip_limits = ip_limits
        self._socket_history = defaultdict(lambda: defaultdict(list))
        self._ip_history = defaultdict(lambda: defaultdict(list))

    def check_socket(self, sid, event):
        """Socket.IO event rate limit ellenőrzés. True = engedélyezve."""
        if event not in self._socket_limits:
            return True
        max_requests, window = self._socket_limits[event]
        now = time.time()
        timestamps = self._socket_history[sid][event]
        self._socket_history[sid][event] = [t for t in timestamps if now - t < window]
        if len(self._socket_history[sid][event]) >= max_requests:
            return False
        self._socket_history[sid][event].append(now)
        return True

    def check_ip(self, ip, action):
        """IP-alapú rate limit ellenőrzés (auth endpointokra). True = engedélyezve."""
        if action not in self._ip_limits:
            return True
        max_requests, window = self._ip_limits[action]
        now = time.time()
        timestamps = self._ip_history[ip][action]
        self._ip_history[ip][action] = [t for t in timestamps if now - t < window]
        if not self._ip_history[ip][action] and not any(self._ip_history[ip].values()):
            del self._ip_history[ip]
            return True
        if len(self._ip_history[ip][action]) >= max_requests:
            return False
        self._ip_history[ip][action].append(now)
        return True

    def clear_sid(self, sid):
        """SID törlése disconnect-kor."""
        self._socket_history.pop(sid, None)
