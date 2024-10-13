use crate::Game;
use fyrox::{
    core::{
        algebra::{Vector2, Vector3},
        pool::Handle,
        reflect::prelude::*,
        type_traits::prelude::*,
        variable::InheritableVariable,
        visitor::prelude::*,
    },
    event::Event,
    graph::{BaseSceneGraph, SceneGraph},
    scene::{
        animation::spritesheet::SpriteSheetAnimation,
        dim2::{
            collider::Collider, physics::RayCastOptions, rectangle::Rectangle, rigidbody::RigidBody,
        },
        node::Node,
        rigidbody::RigidBodyType,
    },
    script::{ScriptContext, ScriptDeinitContext, ScriptTrait},
};

#[derive(Visit, Default, Reflect, Debug, Clone, TypeUuidProvider, ComponentProvider)]
#[type_uuid(id = "b1e8d6c1-21d2-4355-97a8-22dc5402365d")]
#[visit(optional)]
pub struct Bot {
    rectangle: InheritableVariable<Handle<Node>>,
    speed: InheritableVariable<f32>,
    direction: f32,
    front_obstacle_sensor: InheritableVariable<Handle<Node>>,
    back_obstacle_sensor: InheritableVariable<Handle<Node>>,
    ground_probe: InheritableVariable<Handle<Node>>,
    ground_probe_distance: InheritableVariable<f32>,
    ground_probe_timeout: f32,
    target: Handle<Node>,
    target_timeout: f32,

    animations: Vec<SpriteSheetAnimation>,
    current_animation: InheritableVariable<u32>,
}

impl Bot {
    fn do_move(&mut self, ctx: &mut ScriptContext) {
        let Some(rigid_body) = ctx.scene.graph.try_get_mut_of_type::<RigidBody>(ctx.handle) else {
            return;
        };

        let y_vel = rigid_body.lin_vel().y;

        rigid_body.set_lin_vel(Vector2::new(-*self.speed * self.direction, y_vel));

        // Also, inverse the sprite along the X axis.
        let Some(rectangle) = ctx.scene.graph.try_get_mut(*self.rectangle) else {
            return;
        };

        rectangle.local_transform_mut().set_scale(Vector3::new(
            2.0 * self.direction.signum(),
            2.0,
            1.0,
        ));
    }

    fn has_obstacles(&mut self, ctx: &mut ScriptContext) -> bool {
        // Select the sensor using current walking direction.
        let graph = &ctx.scene.graph;

        let sensor_handle = if self.direction < 0.0 {
            *self.back_obstacle_sensor
        } else {
            *self.front_obstacle_sensor
        };

        // Check if it intersects something.
        let Some(obstacle_sensor) = graph.try_get_of_type::<Collider>(sensor_handle) else {
            return false;
        };

        for intersection in obstacle_sensor
            .intersects(&ctx.scene.graph.physics2d)
            .filter(|i| i.has_any_active_contact)
        {
            for collider_handle in [intersection.collider1, intersection.collider2] {
                let Some(other_collider) = graph.try_get_of_type::<Collider>(collider_handle)
                else {
                    continue;
                };

                let Some(rigid_body) = graph.try_get_of_type::<RigidBody>(other_collider.parent())
                else {
                    continue;
                };

                if rigid_body.body_type() == RigidBodyType::Static {
                    return true;
                }
            }
        }

        false
    }

    fn has_ground_in_front(&self, ctx: &ScriptContext) -> bool {
        // Do ground check using ray casting from the ground probe position down at some distance.
        let Some(ground_probe) = ctx.scene.graph.try_get(*self.ground_probe) else {
            return false;
        };

        let ground_probe_position = ground_probe.global_position().xy();

        let mut intersections = Vec::new();
        ctx.scene.graph.physics2d.cast_ray(
            RayCastOptions {
                ray_origin: ground_probe_position.into(),
                // Cast the ray
                ray_direction: Vector2::new(0.0, -*self.ground_probe_distance),
                max_len: *self.ground_probe_distance,
                groups: Default::default(),
                // Make sure the closest intersection will be first in the list of intersections.
                sort_results: true,
            },
            &mut intersections,
        );

        for intersection in intersections {
            let Some(collider) = ctx.scene.graph.try_get(intersection.collider) else {
                continue;
            };

            let Some(rigid_body) = ctx
                .scene
                .graph
                .try_get_of_type::<RigidBody>(collider.parent())
            else {
                continue;
            };

            if rigid_body.body_type() == RigidBodyType::Static
                && intersection
                    .position
                    .coords
                    .metric_distance(&ground_probe_position)
                    <= *self.ground_probe_distance
            {
                return true;
            }
        }

        false
    }

    fn search_target(&mut self, ctx: &mut ScriptContext) {
        let game = ctx.plugins.get::<Game>();

        let self_position = ctx.scene.graph[ctx.handle].global_position();

        let Some(player) = ctx.scene.graph.try_get(game.player) else {
            return;
        };

        let player_position = player.global_position();

        self.target_timeout -= ctx.dt;
        let signed_distance = player_position.x - self_position.x;
        if signed_distance.abs() < 2.0 && signed_distance.signum() != self.direction.signum() {
            self.target = game.player;
            self.target_timeout = 2.;
        }

        if self.target_timeout <= 0. {
            self.target = Handle::NONE;
        }
    }
}

impl ScriptTrait for Bot {
    fn on_init(&mut self, _context: &mut ScriptContext) {
        // Put initialization logic here.
    }

    fn on_start(&mut self, _context: &mut ScriptContext) {
        // There should be a logic that depends on other scripts in scene.
        // It is called right after **all** scripts were initialized.
    }

    fn on_deinit(&mut self, _context: &mut ScriptDeinitContext) {
        // Put de-initialization logic here.
    }

    fn on_os_event(&mut self, _event: &Event<()>, _context: &mut ScriptContext) {
        // Respond to OS events here.
    }

    fn on_update(&mut self, context: &mut ScriptContext) {
        self.search_target(context);
        self.do_move(context);

        let has_obstacles = self.has_obstacles(context);

        if has_obstacles {
            self.direction = -self.direction;
        }

        self.ground_probe_timeout -= context.dt;
        if self.ground_probe_timeout <= 0.0 {
            if !self.has_ground_in_front(context) {
                self.direction = -self.direction;
            }
            self.ground_probe_timeout = 0.3;
        }

        if self.target.is_some() {
            let target_position = context.scene.graph[self.target].global_position();
            let self_position = context.scene.graph[context.handle].global_position();
            self.direction = (self_position.x - target_position.x).signum();

            // Stand still while attacking.
            if target_position.metric_distance(&self_position) > 1.1 {
                self.speed.set_value_and_mark_modified(1.2);
            } else {
                self.speed.set_value_and_mark_modified(0.0);
            }
        }

        if *self.speed != 0.0 {
            self.current_animation.set_value_and_mark_modified(2);
        }

        if self.target.is_some() {
            let target_position = context.scene.graph[self.target].global_position();
            let self_position = context.scene.graph[context.handle].global_position();
            if target_position.metric_distance(&self_position) < 1.1 {
                self.current_animation.set_value_and_mark_modified(0);
            } else if has_obstacles {
                self.current_animation.set_value_and_mark_modified(3);
            }
        }

        if let Some(current_animation) = self.animations.get_mut(*self.current_animation as usize) {
            current_animation.update(context.dt);

            if let Some(sprite) = context
                .scene
                .graph
                .try_get_mut_of_type::<Rectangle>(*self.rectangle)
            {
                // Set new frame to the sprite.
                sprite
                    .material()
                    .data_ref()
                    .set_texture(&"diffuseTexture".into(), current_animation.texture())
                    .unwrap();
                sprite.set_uv_rect(
                    current_animation
                        .current_frame_uv_rect()
                        .unwrap_or_default(),
                );
            }
        }
    }
}
