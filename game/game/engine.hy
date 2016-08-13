(import [cocos :as c]
        [numpy :as np]
        [game.geo :as geo]
        [game.color :as color]
        [game.constants :as const]
        pyglet
        [scipy.misc [toimage]]
        PIL)

(defn ->str [&rest xs]
  (.join " " (list (map str xs))))

(defn temperature->str [t]
  (let [t* (str (int (round (* t 50))))]
    (+ t* "°C")))

(defn height->str [h]
  (let [h* (str (int (round (* 4000 (get h 0)))))]
    (+ h* "m")))

(defn biome->str [b]
  (or (get const.pretty-biomes b) "N/A"))

(defn coords->str [c]
  (.join ";" (map str (map int c))))

(defn get-diag-text [context [x y]]
  (when context
    (let [coords (, (- 512 y) x)
          height (:height context)
          biomes (:biomes context)
          temperature (:temperature context)]
      (->str (coords->str coords)
             (biome->str (get biomes coords))
             (height->str (get height coords))
             (temperature->str (get temperature coords))))))

(defclass DiagLayer [c.layer.Layer]
  (setv is_event_handler true)
  (defn --init-- [self context]
    (.--init-- (super DiagLayer self))
    (setv self.context context)
    (let [label (c.text.Label "Welcome to Calliope" :position (, 10 10) :color (, 10 10 10 255))]
      (.add self label)
      (setv self.label label)))
  (defn update-label [self new-text]
    (when new-text
      (setv self.label.element.text new-text)))
  (defn on-mouse-event [self x y]
    (let [coords (c.director.director.get-virtual-coordinates x y)
          text (get-diag-text self.context coords)]
      (self.update-label text)))
  (defn on-mouse-motion [self x y _ __]
    (self.on-mouse-event x y))
  (defn on-mouse-press [self x y _ __]
    (self.on-mouse-event x y)))

(defclass MapLayer [c.layer.Layer])

(defn start []
  (c.director.director.init :width 512 :height 512)
  (let [diag-layer (DiagLayer nil)
        l (geo.generate-layers)
        map-layer (create-map-layer l)
        main-scene (c.scene.Scene map-layer diag-layer)]
    (setv diag-layer.context l)
    (c.director.director.run main-scene)))

(defn np->texture [npa]
  (let [[_ x y] npa.shape]
    (pyglet.image.ImageData x y "RGB" (.tobytes (toimage npa)))))

(defn generate-world-sprite [l]
  (let [pixs (np->texture (np.array (color.world->pixels (:biomes l))))]
    (c.sprite.Sprite pixs :anchor (, 0 0))))

(defn create-map-layer [l]
  (let [s (generate-world-sprite l)
        ml (MapLayer)]
    (.add ml s)))
