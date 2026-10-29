#pragma once
#include <algorithm>
#include <cmath>

// Physical coordinates; only distances/timing constants originate in logical pixels.
namespace edge_dock {
enum class Edge { None, Left, Right };
enum class Phase { Free, Dragging, Snapping, Full, Hiding, Half, Revealing, Suspended };
struct Point { double x = 0, y = 0; };
struct Monitor {
    double left, top, right, bottom, workTop, workBottom, scale;
    bool valid() const {
        return std::isfinite(scale) && scale > 0 && right - left >= 104 * scale
            && workBottom - workTop >= 104 * scale;
    }
};
struct Geometry {
    Monitor monitor;
    // The ball has radius 40 and its center is (52,52) in a 104px window.
    double center() const { return 52 * monitor.scale; }
    double radius() const { return 40 * monitor.scale; }
    double margin() const { return center() - radius(); }
    Point full(Edge edge, double y) const {
        return {edge == Edge::Left ? monitor.left + 8 * monitor.scale - margin()
            : monitor.right - 8 * monitor.scale - center() - radius(),
            std::clamp(y, monitor.workTop, monitor.workBottom - 104 * monitor.scale)};
    }
    Point half(Edge edge, double y) const {
        return {(edge == Edge::Left ? monitor.left : monitor.right) - center(), full(edge, y).y};
    }
    Point recover(Point p) const {
        return {std::clamp(p.x, monitor.left, monitor.right - 104 * monitor.scale),
            std::clamp(p.y, monitor.workTop, monitor.workBottom - 104 * monitor.scale)};
    }
    Edge nearest(Point p) const {
        const double left = std::abs(p.x + margin() - monitor.left);
        const double right = std::abs(monitor.right - (p.x + center() + radius()));
        if (std::min(left, right) >= 56 * monitor.scale) return Edge::None;
        return left <= right ? Edge::Left : Edge::Right;
    }
};

// One owner (the native window thread), one animation and one deadline.
// No UI timer is allowed to move the HWND independently of this controller.
class Controller {
public:
    Phase phase = Phase::Free;
    Edge edge = Edge::None;
    Geometry geometry{};
    Point position{};
    bool enabled = false, reduced = false;
    bool halfHide = true, hoverReveal = true;

    void configure(bool value, double now) {
        enabled = value;
        if (!enabled) {
            if (edge != Edge::None && geometry.monitor.valid()) position = geometry.full(edge, position.y);
            edge = Edge::None;
            phase = Phase::Free;
            deadline = 0;
        }
        (void)now;
    }
    void restore(Edge saved, Geometry g, double now) {
        geometry = g;
        if (!enabled || !g.monitor.valid() || saved == Edge::None) return;
        edge = saved;
        position = geometry.full(edge, position.y);
        phase = Phase::Full;
        deadline = now + 600;
    }
    void suspend() { phase = Phase::Suspended; deadline = 0; }
    void resume(double now) {
        if (edge != Edge::None && enabled) {
            position = geometry.full(edge, position.y);
            phase = Phase::Full;
            deadline = now + 600;
        } else phase = Phase::Free;
    }
    void press() { phase = Phase::Dragging; deadline = 0; }
    void undock(Geometry g) {
        if (edge != Edge::None && g.monitor.valid()) position = g.recover(position);
        geometry = g;
        edge = Edge::None;
    }
    void release(Geometry g, double now) {
        geometry = g;
        edge = enabled && g.monitor.valid() ? g.nearest(position) : Edge::None;
        if (edge == Edge::None) { phase = Phase::Free; return; }
        start(Phase::Snapping, g.full(edge, position.y), now, 220);
    }
    void recover(Geometry g) {
        if (g.monitor.valid()) position = g.recover(position);
        geometry = g;
        edge = Edge::None;
        phase = Phase::Free;
        deadline = 0;
    }
    Point saved() const { return edge == Edge::None ? position : geometry.full(edge, position.y); }
    void tick(double now, bool hovered) {
        if (!enabled || phase == Phase::Free || phase == Phase::Dragging || phase == Phase::Suspended) return;
        if (!halfHide) {
            position = geometry.full(edge, position.y);
            phase = Phase::Full;
            deadline = 0;
            return;
        }
        if (hovered && hoverReveal && (phase == Phase::Half || phase == Phase::Hiding))
            start(Phase::Revealing, geometry.full(edge, position.y), now, 180);
        if (phase == Phase::Snapping || phase == Phase::Hiding || phase == Phase::Revealing) {
            const double t = std::clamp((now - began) / duration, 0.0, 1.0);
            const double ease = 1 - std::pow(1 - t, 3);
            position = {from.x + (target.x - from.x) * ease, from.y + (target.y - from.y) * ease};
            if (t < 1) return;
            if (phase == Phase::Hiding) phase = Phase::Half;
            else {
                const bool snapped = phase == Phase::Snapping;
                phase = Phase::Full;
                deadline = now + (snapped ? 450 : 800);
            }
        }
        if (phase == Phase::Full) {
            if (hovered) deadline = 0;
            else if (deadline == 0) deadline = now + 800;
            else if (now >= deadline) start(Phase::Hiding, geometry.half(edge, position.y), now, 200);
        }
    }
private:
    Point from{}, target{};
    double began = 0, duration = 1, deadline = 0;
    void start(Phase next, Point to, double now, double ms) {
        phase = next; from = position; target = to; began = now;
        duration = reduced ? 100 : ms;
        deadline = 0;
    }
};
}
