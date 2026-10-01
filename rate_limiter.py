import time
from collections import defaultdict


class RateLimiter:
    """Generikus rate limiter Socket.IO (SID) és HTTP (IP) kérésekhez."""

    _IP_PRUNE_THRESHOLD = 1000

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
        if len(self._ip_history) > self._IP_PRUNE_THRESHOLD:
            self._prune_ip_history(now)
        history = self._ip_history[ip]
        history[action] = [t for t in history[action] if now - t < window]
        if len(history[action]) >= max_requests:
            return False
        history[action].append(now)
        return True

    def _prune_ip_history(self, now):
        """Törli a már lejárt időbélyegű IP bejegyzéseket (memória-növekedés ellen)."""
        longest_window = max((w for _, w in self._ip_limits.values()), default=0)
        for ip in list(self._ip_history):
            actions = self._ip_history[ip]
            if not any(t for stamps in actions.values() for t in stamps
                       if now - t < longest_window):
                del self._ip_history[ip]

    def clear_sid(self, sid):
        """SID törlése disconnect-kor."""
        self._socket_history.pop(sid, None)
